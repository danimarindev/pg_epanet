use pgrx::prelude::*;

mod builder;
mod epanet_sections;
mod export;
mod ffi;
mod inp;
mod map;
mod metadata;
mod scenario;
mod schema;
mod temp;
mod topology;
mod validate;

::pgrx::pg_module_magic!(name, version);

#[pg_guard]
unsafe extern "C-unwind" fn _PG_init() {
    temp::register_gucs();
}

#[pg_extern]
fn hello_pg_epanet() -> &'static str {
    "Hello, pg_epanet"
}

/// Returns rows from the [RESERVOIRS] section of an INP file.
#[pg_extern]
fn epanet_reservoirs(
    inp_text: &str,
) -> TableIterator<'static, (name!(name, String), name!(head, f64), name!(pattern, Option<String>))>
{
    let mut sections = inp::parse_sections(inp_text);
    let rows = sections.remove("RESERVOIRS").unwrap_or_default();

    TableIterator::new(rows.into_iter().filter_map(|fields| {
        if fields.len() < 2 {
            return None;
        }
        let name = fields[0].clone();
        let head: f64 = fields[1].parse().ok()?;
        let pattern = fields.get(2).cloned();
        Some((name, head, pattern))
    }))
}

/// Returns rows from the [TANKS] section of an INP file.
/// min_volume defaults to 0.0 when absent.
/// volume_curve is NULL when absent or when the value is '*'.
#[pg_extern]
fn epanet_tanks(
    inp_text: &str,
) -> TableIterator<
    'static,
    (
        name!(name, String),
        name!(elevation, f64),
        name!(init_level, f64),
        name!(min_level, f64),
        name!(max_level, f64),
        name!(diameter, f64),
        name!(min_volume, f64),
        name!(volume_curve, Option<String>),
        name!(overflow, Option<String>),
    ),
>
{
    let mut sections = inp::parse_sections(inp_text);
    let rows = sections.remove("TANKS").unwrap_or_default();

    TableIterator::new(rows.into_iter().filter_map(|fields| {
        if fields.len() < 6 {
            return None;
        }
        let name = fields[0].clone();
        let elevation: f64 = fields[1].parse().ok()?;
        let init_level: f64 = fields[2].parse().ok()?;
        let min_level: f64 = fields[3].parse().ok()?;
        let max_level: f64 = fields[4].parse().ok()?;
        let diameter: f64 = fields[5].parse().ok()?;
        let min_volume: f64 = if fields.len() >= 7 {
            fields[6].parse().ok()?
        } else {
            0.0
        };
        let volume_curve: Option<String> = fields.get(7).and_then(|s| {
            if s == "*" { None } else { Some(s.clone()) }
        });
        let overflow: Option<String> = fields.get(8).cloned();
        Some((name, elevation, init_level, min_level, max_level, diameter, min_volume, volume_curve, overflow))
    }))
}

pub(crate) fn sql_text(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}

/// Parses an EPANET INP file and materialises it into the `epanet` schema with PostGIS geometry.
/// When `replace` is true, deletes any existing networks with the same name before importing.
/// Returns the assigned `network_id`.
#[pg_extern]
fn epanet_import(
    network_name: &str,
    inp_text: &str,
    srid: default!(i32, "5367"),
    replace: default!(bool, false),
) -> i32 {
    if replace {
        Spi::run(&format!(
            "DELETE FROM epanet.networks WHERE name = {}",
            sql_text(network_name)
        ))
        .unwrap_or_else(|e| error!("SPI error deleting existing network: {e:?}"));
    }

    let lit = sql_text(inp_text);
    let network_id = Spi::get_one::<i32>(&format!(
        "INSERT INTO epanet.networks(name, srid, inp_text) VALUES ({}, {srid}, {lit}) RETURNING id",
        sql_text(network_name)
    ))
    .unwrap()
    .unwrap();

    let nid = network_id;

    // Populate tables via the existing epanet_* table-returning functions
    for (table, func, cols) in [
        ("junctions",   "epanet_junctions",   "name, elevation, demand, pattern"),
        ("reservoirs",  "epanet_reservoirs",  "name, head, pattern"),
        ("tanks",       "epanet_tanks",       "name, elevation, init_level, min_level, max_level, diameter, min_volume, volume_curve, overflow"),
        ("pipes",       "epanet_pipes",       "name, node1, node2, length, diameter, roughness, minor_loss, status"),
        ("pumps",       "epanet_pumps",       "name, node1, node2, pump_type, head_curve, power, speed, pattern"),
        ("valves",      "epanet_valves",      "name, node1, node2, diameter, valve_type, setting, minor_loss"),
        ("coordinates", "epanet_coordinates", "node_id, x, y"),
    ] {
        Spi::run(&format!(
            "INSERT INTO epanet.{table}(network_id, {cols}) SELECT {nid}, * FROM {func}({lit})"
        ))
        .unwrap();
    }

    // Vertices with an ordering index to guarantee correct LineString point order
    let mut sections = inp::parse_sections(inp_text);
    let vertex_rows = sections.remove("VERTICES").unwrap_or_default();
    if !vertex_rows.is_empty() {
        let mut link_idx: std::collections::HashMap<String, i32> = std::collections::HashMap::new();
        let values: Vec<String> = vertex_rows
            .iter()
            .filter_map(|f| {
                if f.len() != 3 { return None; }
                let x: f64 = f[1].parse().ok()?;
                let y: f64 = f[2].parse().ok()?;
                let idx = link_idx.entry(f[0].clone()).or_insert(0);
                let row = format!("({nid},{},{idx},{x},{y})", sql_text(&f[0]));
                *idx += 1;
                Some(row)
            })
            .collect();
        if !values.is_empty() {
            Spi::run(&format!(
                "INSERT INTO epanet.vertices(network_id, link_id, idx, x, y) VALUES {}",
                values.join(",")
            ))
            .unwrap();
        }
    }

    // Point geometry for nodes (junctions, tanks, reservoirs)
    Spi::run(&format!(
        "UPDATE epanet.junctions t \
         SET geom = ST_SetSRID(ST_MakePoint(c.x, c.y), {srid}) \
         FROM epanet.coordinates c \
         WHERE t.network_id = c.network_id AND t.name = c.node_id AND t.network_id = {nid}; \
         UPDATE epanet.tanks t \
         SET geom = ST_SetSRID(ST_MakePoint(c.x, c.y), {srid}) \
         FROM epanet.coordinates c \
         WHERE t.network_id = c.network_id AND t.name = c.node_id AND t.network_id = {nid}; \
         UPDATE epanet.reservoirs t \
         SET geom = ST_SetSRID(ST_MakePoint(c.x, c.y), {srid}) \
         FROM epanet.coordinates c \
         WHERE t.network_id = c.network_id AND t.name = c.node_id AND t.network_id = {nid}"
    ))
    .unwrap();

    // LineString geometry for pipes: node1 + ordered intermediate vertices + node2
    Spi::run(&format!(
        "UPDATE epanet.pipes p \
         SET geom = ST_SetSRID( \
             ST_MakeLine( \
                 ARRAY[ST_MakePoint(c1.x, c1.y)] \
                 || ARRAY(SELECT ST_MakePoint(v.x, v.y) \
                          FROM epanet.vertices v \
                          WHERE v.network_id = p.network_id AND v.link_id = p.name \
                          ORDER BY v.idx) \
                 || ARRAY[ST_MakePoint(c2.x, c2.y)] \
             ), {srid}) \
         FROM epanet.coordinates c1, epanet.coordinates c2 \
         WHERE p.network_id = c1.network_id AND p.node1 = c1.node_id \
           AND p.network_id = c2.network_id AND p.node2 = c2.node_id \
           AND p.network_id = {nid}"
    ))
    .unwrap();

    // Direct node1→node2 LineString geometry for pumps and valves
    Spi::run(&format!(
        "UPDATE epanet.pumps lnk \
         SET geom = ST_SetSRID(ST_MakeLine(ST_MakePoint(c1.x, c1.y), ST_MakePoint(c2.x, c2.y)), {srid}) \
         FROM epanet.coordinates c1, epanet.coordinates c2 \
         WHERE lnk.network_id = c1.network_id AND lnk.node1 = c1.node_id \
           AND lnk.network_id = c2.network_id AND lnk.node2 = c2.node_id \
           AND lnk.network_id = {nid}; \
         UPDATE epanet.valves lnk \
         SET geom = ST_SetSRID(ST_MakeLine(ST_MakePoint(c1.x, c1.y), ST_MakePoint(c2.x, c2.y)), {srid}) \
         FROM epanet.coordinates c1, epanet.coordinates c2 \
         WHERE lnk.network_id = c1.network_id AND lnk.node1 = c1.node_id \
           AND lnk.network_id = c2.network_id AND lnk.node2 = c2.node_id \
           AND lnk.network_id = {nid}"
    ))
    .unwrap();

    metadata::import_metadata_sections(network_id, inp_text);

    network_id
}

/// Reads an INP file from the Postgres server filesystem and imports it.
/// Requires superuser (server-side file access).
#[pg_extern]
fn epanet_import_file(
    network_name: &str,
    file_path: &str,
    srid: default!(i32, "5367"),
    replace: default!(bool, false),
) -> i32 {
    let is_super = Spi::get_one::<bool>(
        "SELECT COALESCE((SELECT rolsuper FROM pg_roles WHERE rolname = current_user), false)",
    )
    .unwrap()
    .unwrap_or(false);
    if !is_super {
        error!("epanet_import_file requires superuser privileges");
    }
    let content = std::fs::read_to_string(file_path)
        .unwrap_or_else(|e| error!("Cannot read file {file_path}: {e}"));
    epanet_import(network_name, &content, srid, replace)
}

/// Regenerates a valid EPANET INP from stored tables.
#[pg_extern]
fn epanet_export(network_id: i32) -> String {
    export::export_network(network_id)
}

/// Rebuilds `networks.inp_text` from current table state.
#[pg_extern]
fn epanet_refresh_inp(network_id: i32) -> bool {
    export::refresh_inp_text(network_id);
    true
}

/// Validates topology and reference integrity for a network.
#[pg_extern]
fn epanet_validate(
    network_id: i32,
) -> TableIterator<
    'static,
    (
        name!(severity, String),
        name!(issue_type, String),
        name!(object_type, String),
        name!(object_id, String),
        name!(message, String),
    ),
> {
    let issues = validate::validate_network(network_id);
    TableIterator::new(issues.into_iter().map(|i| {
        (
            i.severity,
            i.issue_type,
            i.object_type,
            i.object_id,
            i.message,
        )
    }))
}

fn epanet_error_message(ec: i32) -> String {
    use crate::ffi::*;
    use std::ffi::{CStr, c_char};
    unsafe {
        let mut buf = vec![0 as c_char; 256];
        EN_geterror(ec, buf.as_mut_ptr(), 255);
        CStr::from_ptr(buf.as_ptr())
            .to_string_lossy()
            .into_owned()
    }
}

fn format_epanet_f64(v: f64) -> String {
    if v.is_finite() {
        format!("{v}")
    } else {
        "NULL".into()
    }
}

/// Reads the immutable base INP snapshot stored at import time.
/// Simulations never mutate `networks.inp_text`; use scenarios for what-if changes.
fn base_inp_for_network(network_id: i32) -> String {
    Spi::get_one::<String>(&format!(
        "SELECT inp_text FROM epanet.networks WHERE id = {network_id}"
    ))
    .unwrap()
    .unwrap_or_else(|| error!("No network found with id={network_id}"))
}

fn run_hydraulic_eps(
    inp_text: &str,
    network_id: i32,
    scenario_id: Option<i32>,
    file_prefix: &str,
) -> i32 {
    use crate::ffi::*;
    use std::ffi::{CStr, c_char};

    let files = temp::TempProjectFiles::new(file_prefix);
    files.write_inp(inp_text);

    let c_inp = temp::path_to_cstring(&files.inp_path);
    let c_rpt = temp::path_to_cstring(&files.rpt_path);
    let c_out = temp::path_to_cstring(&files.out_path);

    let ph = unsafe {
        let mut ph: EN_Project = std::ptr::null_mut();
        let ec = EN_createproject(&mut ph);
        if ec != 0 {
            error!("EN_createproject failed (code {})", ec);
        }
        let ec = EN_open(ph, c_inp.as_ptr(), c_rpt.as_ptr(), c_out.as_ptr());
        if ec != 0 && ec != 200 {
            EN_deleteproject(ph);
            error!(
                "EN_open failed (code {ec}): {}",
                epanet_error_message(ec)
            );
        }
        ph
    };

    let (n_nodes, n_links) = unsafe {
        let mut nn: i32 = 0;
        let mut nl: i32 = 0;
        EN_getcount(ph, EN_NODECOUNT, &mut nn);
        EN_getcount(ph, EN_LINKCOUNT, &mut nl);
        (nn, nl)
    };

    let open_ec = unsafe { EN_openH(ph) };
    if open_ec >= 100 {
        unsafe {
            EN_close(ph);
            EN_deleteproject(ph);
        }
        error!("EN_openH failed (code {})", open_ec);
    }
    unsafe { EN_initH(ph, EN_NOSAVE) };

    let mut node_parts: Vec<String> = Vec::new();
    let mut link_parts: Vec<String> = Vec::new();
    let mut n_steps: i32 = 0;

    loop {
        let mut current_time: i64 = 0;
        let ec = unsafe { EN_runH(ph, &mut current_time) };
        if ec >= 100 {
            unsafe {
                EN_closeH(ph);
                EN_close(ph);
                EN_deleteproject(ph);
            }
            error!("EN_runH failed (code {}) at t={}s", ec, current_time);
        }
        if (1..100).contains(&ec) {
            warning!(
                "epanet: solver warning {} at t={}s: {}",
                ec,
                current_time,
                epanet_error_message(ec)
            );
        }

        let step = n_steps;

        unsafe {
            let mut buf = vec![0 as c_char; 64];
            for i in 1..=n_nodes {
                EN_getnodeid(ph, i, buf.as_mut_ptr());
                let mut head: f64 = 0.0;
                let mut pressure: f64 = 0.0;
                let mut demand: f64 = 0.0;
                EN_getnodevalue(ph, i, EN_HEAD, &mut head);
                EN_getnodevalue(ph, i, EN_PRESSURE, &mut pressure);
                EN_getnodevalue(ph, i, EN_DEMAND, &mut demand);
                let name = CStr::from_ptr(buf.as_ptr())
                    .to_string_lossy()
                    .replace('\'', "''");
                let hv = format_epanet_f64(head);
                let pv = format_epanet_f64(pressure);
                let dv = format_epanet_f64(demand);
                node_parts.push(format!("{step},'{name}',{hv},{pv},{dv}"));
            }
        }

        unsafe {
            let mut buf = vec![0 as c_char; 64];
            for i in 1..=n_links {
                EN_getlinkid(ph, i, buf.as_mut_ptr());
                let mut flow: f64 = 0.0;
                let mut velocity: f64 = 0.0;
                let mut headloss: f64 = 0.0;
                EN_getlinkvalue(ph, i, EN_FLOW, &mut flow);
                EN_getlinkvalue(ph, i, EN_VELOCITY, &mut velocity);
                EN_getlinkvalue(ph, i, EN_HEADLOSS, &mut headloss);
                let name = CStr::from_ptr(buf.as_ptr())
                    .to_string_lossy()
                    .replace('\'', "''");
                let fv = format_epanet_f64(flow);
                let vv = format_epanet_f64(velocity);
                let lv = format_epanet_f64(headloss);
                link_parts.push(format!("{step},'{name}',{fv},{vv},{lv}"));
            }
        }

        n_steps += 1;

        let mut t_step: i64 = 0;
        unsafe { EN_nextH(ph, &mut t_step) };
        if t_step <= 0 {
            break;
        }
    }

    unsafe {
        EN_closeH(ph);
        EN_close(ph);
        EN_deleteproject(ph);
    }

    let scenario_sql = match scenario_id {
        Some(id) => id.to_string(),
        None => "NULL".into(),
    };

    let run_id = Spi::get_one::<i32>(&format!(
        "INSERT INTO epanet.simulation_runs(network_id, scenario_id, n_steps) \
         VALUES ({network_id}, {scenario_sql}, {n_steps}) RETURNING id"
    ))
    .unwrap_or_else(|e| error!("SPI error inserting simulation_run: {e:?}"))
    .unwrap();

    if !node_parts.is_empty() {
        let vals: String = node_parts
            .iter()
            .map(|r| format!("({run_id},{r})"))
            .collect::<Vec<_>>()
            .join(",");
        Spi::run(&format!(
            "INSERT INTO epanet.node_results(run_id,step,node_id,head,pressure,demand) \
             VALUES {vals}"
        ))
        .unwrap_or_else(|e| error!("SPI error inserting node_results: {e:?}"));
    }

    if !link_parts.is_empty() {
        let vals: String = link_parts
            .iter()
            .map(|r| format!("({run_id},{r})"))
            .collect::<Vec<_>>()
            .join(",");
        Spi::run(&format!(
            "INSERT INTO epanet.link_results(run_id,step,link_id,flow,velocity,headloss) \
             VALUES {vals}"
        ))
        .unwrap_or_else(|e| error!("SPI error inserting link_results: {e:?}"));
    }

    run_id
}

/// Creates a scenario for what-if studies. Does not modify the base network or INP.
#[pg_extern]
fn epanet_create_scenario(
    network_id: i32,
    name: &str,
    description: default!(Option<&str>, NULL),
    demand_multiplier: default!(f64, 1.0),
) -> i32 {
    let desc_sql = match description {
        Some(d) => sql_text(d),
        None => "NULL".into(),
    };
    Spi::get_one::<i32>(&format!(
        "INSERT INTO epanet.scenarios(network_id, name, description, demand_multiplier) \
         VALUES ({network_id}, {}, {desc_sql}, {demand_multiplier}) RETURNING id",
        sql_text(name)
    ))
    .unwrap_or_else(|e| error!("SPI error creating scenario: {e:?}"))
    .unwrap()
}

/// Sets or replaces a scenario override (base network remains unchanged).
#[pg_extern]
fn epanet_set_scenario_override(
    scenario_id: i32,
    target_type: &str,
    target_id: &str,
    parameter: &str,
    value: &str,
) -> bool {
    Spi::run(&format!(
        "INSERT INTO epanet.scenario_overrides(scenario_id, target_type, target_id, parameter, value) \
         VALUES ({scenario_id}, {}, {}, {}, {}) \
         ON CONFLICT (scenario_id, target_type, target_id, parameter) \
         DO UPDATE SET value = EXCLUDED.value",
        sql_text(target_type),
        sql_text(target_id),
        sql_text(parameter),
        sql_text(value)
    ))
    .unwrap_or_else(|e| error!("SPI error setting scenario override: {e:?}"));
    true
}

#[pg_extern]
fn epanet_delete_scenario(scenario_id: i32) -> bool {
    Spi::get_one::<bool>(&format!(
        "DELETE FROM epanet.scenarios WHERE id = {scenario_id} RETURNING true"
    ))
    .unwrap_or_else(|e| error!("SPI error deleting scenario: {e:?}"))
    .unwrap_or_else(|| error!("No scenario found with id={scenario_id}"))
}

/// Convenience: scenario with a single pipe closed (pipe break / criticality study).
#[pg_extern]
fn epanet_scenario_pipe_closure(network_id: i32, name: &str, pipe_id: &str) -> i32 {
    let sid = epanet_create_scenario(network_id, name, None, 1.0);
    epanet_set_scenario_override(sid, "pipe", pipe_id, "status", "Closed");
    sid
}

/// Convenience: fire-flow check via extra demand at a junction.
#[pg_extern]
fn epanet_scenario_fire_flow(
    network_id: i32,
    name: &str,
    junction_id: &str,
    required_flow: f64,
) -> i32 {
    let sid = epanet_create_scenario(network_id, name, None, 1.0);
    epanet_set_scenario_override(
        sid,
        "junction",
        junction_id,
        "demand",
        &required_flow.to_string(),
    );
    sid
}

/// Runs EPS using base INP + scenario overrides (never mutates the stored network).
#[pg_extern]
fn epanet_simulate_scenario(scenario_id: i32) -> i32 {
    let (network_id, inp_text) = scenario::effective_inp_for_scenario(scenario_id);
    run_hydraulic_eps(
        &inp_text,
        network_id,
        Some(scenario_id),
        &format!("pg_epanet_scenario_{scenario_id}"),
    )
}

/// Compares two simulation runs (node pressure and link flow deltas per timestep).
#[pg_extern]
fn epanet_compare_runs(
    run_id_a: i32,
    run_id_b: i32,
) -> TableIterator<
    'static,
    (
        name!(result_kind, String),
        name!(element_id, String),
        name!(step, i32),
        name!(metric, String),
        name!(value_a, Option<f64>),
        name!(value_b, Option<f64>),
        name!(delta, Option<f64>),
    ),
> {
    use pgrx::spi::SpiResult;
    let mut rows: Vec<(String, String, i32, String, Option<f64>, Option<f64>, Option<f64>)> =
        Vec::new();

    let _ = Spi::connect(|client| -> SpiResult<_> {
        let q = client.select(
            &format!(
                "SELECT 'node'::text AS kind, a.node_id AS eid, a.step, 'pressure'::text AS metric, \
                        a.pressure AS va, b.pressure AS vb, a.pressure - b.pressure AS delta \
                 FROM epanet.node_results a \
                 JOIN epanet.node_results b \
                   ON a.node_id = b.node_id AND a.step = b.step \
                 WHERE a.run_id = {run_id_a} AND b.run_id = {run_id_b} \
                 UNION ALL \
                 SELECT 'link', a.link_id, a.step, 'flow', a.flow, b.flow, a.flow - b.flow \
                 FROM epanet.link_results a \
                 JOIN epanet.link_results b \
                   ON a.link_id = b.link_id AND a.step = b.step \
                 WHERE a.run_id = {run_id_a} AND b.run_id = {run_id_b} \
                 ORDER BY kind, eid, step, metric"
            ),
            None,
            None,
        )?;
        for row in q {
            rows.push((
                row.get_by_name("kind")?.unwrap(),
                row.get_by_name("eid")?.unwrap(),
                row.get_by_name("step")?.unwrap(),
                row.get_by_name("metric")?.unwrap(),
                row.get_by_name("va")?,
                row.get_by_name("vb")?,
                row.get_by_name("delta")?,
            ));
        }
        Ok(())
    });

    TableIterator::new(rows.into_iter())
}

/// Creates an empty network with default OPTIONS/TIMES/REPORT. Add elements, then `epanet_refresh_inp`.
#[pg_extern]
fn epanet_create_network(network_name: &str, srid: default!(i32, "4326")) -> i32 {
    builder::create_network(network_name, srid)
}

/// Adds a junction to the base network tables (call `epanet_refresh_inp` to update INP text).
#[pg_extern]
fn epanet_add_junction(
    network_id: i32,
    name: &str,
    elevation: f64,
    demand: f64,
    x: f64,
    y: f64,
    pattern: default!(Option<&str>, NULL),
) -> bool {
    topology::add_junction(network_id, name, elevation, demand, x, y, pattern);
    true
}

/// Adds a pipe to the base network tables.
#[pg_extern]
fn epanet_add_pipe(
    network_id: i32,
    name: &str,
    node1: &str,
    node2: &str,
    length: f64,
    diameter: f64,
    roughness: f64,
    minor_loss: default!(f64, 0.0),
    status: default!(&str, "Open"),
) -> bool {
    topology::add_pipe(
        network_id, name, node1, node2, length, diameter, roughness, minor_loss, status,
    );
    true
}

/// Adds a reservoir to the base network tables.
#[pg_extern]
fn epanet_add_reservoir(
    network_id: i32,
    name: &str,
    head: f64,
    x: f64,
    y: f64,
    pattern: default!(Option<&str>, NULL),
) -> bool {
    topology::add_reservoir(network_id, name, head, x, y, pattern);
    true
}

/// Adds a tank to the base network tables.
#[pg_extern]
fn epanet_add_tank(
    network_id: i32,
    name: &str,
    elevation: f64,
    init_level: f64,
    min_level: f64,
    max_level: f64,
    diameter: f64,
    min_volume: default!(f64, 0.0),
    x: f64,
    y: f64,
    volume_curve: default!(Option<&str>, NULL),
    overflow: default!(Option<&str>, NULL),
) -> bool {
    topology::add_tank(
        network_id,
        name,
        elevation,
        init_level,
        min_level,
        max_level,
        diameter,
        min_volume,
        x,
        y,
        volume_curve,
        overflow,
    );
    true
}

/// Adds a pump to the base network tables. `pump_type` is `HEAD` or `POWER`.
#[pg_extern]
fn epanet_add_pump(
    network_id: i32,
    name: &str,
    node1: &str,
    node2: &str,
    pump_type: &str,
    head_curve: default!(Option<&str>, NULL),
    power: default!(Option<f64>, NULL),
    speed: default!(Option<f64>, NULL),
    pattern: default!(Option<&str>, NULL),
) -> bool {
    topology::add_pump(
        network_id,
        name,
        node1,
        node2,
        pump_type,
        head_curve,
        power,
        speed,
        pattern,
    );
    true
}

/// Adds a valve to the base network tables.
#[pg_extern]
fn epanet_add_valve(
    network_id: i32,
    name: &str,
    node1: &str,
    node2: &str,
    diameter: f64,
    valve_type: &str,
    setting: &str,
    minor_loss: default!(f64, 0.0),
) -> bool {
    topology::add_valve(
        network_id, name, node1, node2, diameter, valve_type, setting, minor_loss,
    );
    true
}

/// Adds a provisional junction that exists only in scenario simulations.
#[pg_extern]
fn epanet_add_scenario_junction(
    scenario_id: i32,
    name: &str,
    elevation: f64,
    demand: f64,
    x: f64,
    y: f64,
    pattern: default!(Option<&str>, NULL),
) -> bool {
    topology::add_scenario_junction(scenario_id, name, elevation, demand, x, y, pattern);
    true
}

/// Adds a provisional pipe that exists only in scenario simulations.
#[pg_extern]
fn epanet_add_scenario_pipe(
    scenario_id: i32,
    name: &str,
    node1: &str,
    node2: &str,
    length: f64,
    diameter: f64,
    roughness: f64,
    minor_loss: default!(f64, 0.0),
    status: default!(&str, "Open"),
) -> bool {
    topology::add_scenario_pipe(
        scenario_id, name, node1, node2, length, diameter, roughness, minor_loss, status,
    );
    true
}

/// Adds a provisional reservoir that exists only in scenario simulations.
#[pg_extern]
fn epanet_add_scenario_reservoir(
    scenario_id: i32,
    name: &str,
    head: f64,
    x: f64,
    y: f64,
    pattern: default!(Option<&str>, NULL),
) -> bool {
    topology::add_scenario_reservoir(scenario_id, name, head, x, y, pattern);
    true
}

/// Adds a provisional tank that exists only in scenario simulations.
#[pg_extern]
fn epanet_add_scenario_tank(
    scenario_id: i32,
    name: &str,
    elevation: f64,
    init_level: f64,
    min_level: f64,
    max_level: f64,
    diameter: f64,
    min_volume: default!(f64, 0.0),
    x: f64,
    y: f64,
    volume_curve: default!(Option<&str>, NULL),
    overflow: default!(Option<&str>, NULL),
) -> bool {
    topology::add_scenario_tank(
        scenario_id,
        name,
        elevation,
        init_level,
        min_level,
        max_level,
        diameter,
        min_volume,
        x,
        y,
        volume_curve,
        overflow,
    );
    true
}

/// Adds a provisional pump that exists only in scenario simulations.
#[pg_extern]
fn epanet_add_scenario_pump(
    scenario_id: i32,
    name: &str,
    node1: &str,
    node2: &str,
    pump_type: &str,
    head_curve: default!(Option<&str>, NULL),
    power: default!(Option<f64>, NULL),
    speed: default!(Option<f64>, NULL),
    pattern: default!(Option<&str>, NULL),
) -> bool {
    topology::add_scenario_pump(
        scenario_id,
        name,
        node1,
        node2,
        pump_type,
        head_curve,
        power,
        speed,
        pattern,
    );
    true
}

/// Adds a provisional valve that exists only in scenario simulations.
#[pg_extern]
fn epanet_add_scenario_valve(
    scenario_id: i32,
    name: &str,
    node1: &str,
    node2: &str,
    diameter: f64,
    valve_type: &str,
    setting: &str,
    minor_loss: default!(f64, 0.0),
) -> bool {
    topology::add_scenario_valve(
        scenario_id,
        name,
        node1,
        node2,
        diameter,
        valve_type,
        setting,
        minor_loss,
    );
    true
}

#[pg_extern]
fn epanet_remove_element(network_id: i32, element_type: &str, name: &str) -> bool {
    topology::remove_element(network_id, element_type, name)
}

#[pg_extern]
fn epanet_remove_scenario_element(scenario_id: i32, element_type: &str, name: &str) -> bool {
    topology::remove_scenario_element(scenario_id, element_type, name)
}

#[pg_extern]
fn epanet_connect_nodes(
    network_id: i32,
    link_type: &str,
    link_name: &str,
    node1: &str,
    node2: &str,
) -> bool {
    topology::connect_nodes(network_id, link_type, link_name, node1, node2)
}

/// Promotes scenario elements and overrides into the base network and refreshes INP.
#[pg_extern]
fn epanet_merge_scenario_into_base(scenario_id: i32) -> i32 {
    topology::merge_scenario_into_base(scenario_id)
}

/// Adds or replaces a demand pattern (whitespace-separated multipliers).
#[pg_extern]
fn epanet_add_pattern(network_id: i32, pattern_id: &str, multipliers: &str) -> bool {
    builder::add_pattern(network_id, pattern_id, multipliers);
    true
}

/// Adds or replaces a curve from whitespace-separated x y pairs.
#[pg_extern]
fn epanet_add_curve(network_id: i32, curve_id: &str, xy_pairs: &str) -> bool {
    builder::add_curve(network_id, curve_id, xy_pairs);
    true
}

#[pg_extern]
fn epanet_set_option(network_id: i32, key: &str, value: &str) -> bool {
    builder::set_option(network_id, key, value);
    true
}

#[pg_extern]
fn epanet_set_times(network_id: i32, key: &str, value: &str) -> bool {
    builder::set_times(network_id, key, value);
    true
}

#[pg_extern]
fn epanet_set_report(network_id: i32, key: &str, value: &str) -> bool {
    builder::set_report(network_id, key, value);
    true
}

#[pg_extern]
fn epanet_set_reactions(network_id: i32, key: &str, value: &str) -> bool {
    builder::set_reactions(network_id, key, value);
    true
}

#[pg_extern]
fn epanet_set_quality(network_id: i32, key: &str, value: &str) -> bool {
    builder::set_quality(network_id, key, value);
    true
}

#[pg_extern]
fn epanet_set_energy(network_id: i32, key: &str, value: &str) -> bool {
    builder::set_energy(network_id, key, value);
    true
}

#[pg_extern]
fn epanet_add_control(network_id: i32, rule_text: &str) -> bool {
    builder::add_control(network_id, rule_text);
    true
}

#[pg_extern]
fn epanet_add_rule(network_id: i32, rule_id: &str, rule_text: &str) -> bool {
    builder::add_rule(network_id, rule_id, rule_text);
    true
}

#[pg_extern]
fn epanet_add_demand(
    network_id: i32,
    junction_id: &str,
    demand: f64,
    pattern: default!(Option<&str>, NULL),
) -> bool {
    builder::add_demand(network_id, junction_id, demand, pattern);
    true
}

#[pg_extern]
fn epanet_add_emitter(network_id: i32, junction_id: &str, coefficient: f64) -> bool {
    builder::add_emitter(network_id, junction_id, coefficient);
    true
}

#[pg_extern]
fn epanet_set_link_status(network_id: i32, link_id: &str, status_value: &str) -> bool {
    builder::set_link_status(network_id, link_id, status_value);
    true
}

#[pg_extern]
fn epanet_add_source(
    network_id: i32,
    node_id: &str,
    source_type: &str,
    quality: f64,
    pattern: default!(Option<&str>, NULL),
) -> bool {
    builder::add_source(network_id, node_id, source_type, quality, pattern);
    true
}

#[pg_extern]
fn epanet_add_vertex(network_id: i32, link_id: &str, x: f64, y: f64) -> bool {
    builder::add_vertex(network_id, link_id, x, y);
    true
}

/// Moves a base-network node and refreshes all connected link geometry.
#[pg_extern]
fn epanet_set_node_coordinates(network_id: i32, node_id: &str, x: f64, y: f64) -> bool {
    map::set_node_coordinates(network_id, node_id, x, y);
    true
}

/// Moves a provisional scenario node and refreshes scenario link geometry.
#[pg_extern]
fn epanet_set_scenario_node_coordinates(
    scenario_id: i32,
    node_id: &str,
    x: f64,
    y: f64,
) -> bool {
    topology::set_scenario_node_coordinates(scenario_id, node_id, x, y);
    true
}

/// Adds a bend vertex to a provisional scenario link.
#[pg_extern]
fn epanet_add_scenario_vertex(scenario_id: i32, link_id: &str, x: f64, y: f64) -> bool {
    topology::add_scenario_vertex(scenario_id, link_id, x, y);
    true
}

/// Recomputes stored geometry for all provisional elements in a scenario.
#[pg_extern]
fn epanet_refresh_scenario_geoms(scenario_id: i32) -> bool {
    topology::refresh_scenario_geoms(scenario_id);
    true
}

/// Applies a WKT LineString to an existing pipe (interior points become vertices).
#[pg_extern]
fn epanet_apply_pipe_shape(network_id: i32, pipe_name: &str, wkt: &str) -> bool {
    map::apply_pipe_linestring(network_id, pipe_name, wkt);
    true
}

/// Deletes a network and all associated topology and simulation results (via CASCADE).
/// Returns true when a row was deleted; errors if `network_id` does not exist.
#[pg_extern]
fn epanet_delete(network_id: i32) -> bool {
    Spi::get_one::<bool>(&format!(
        "DELETE FROM epanet.networks WHERE id = {network_id} RETURNING true"
    ))
    .unwrap_or_else(|e| error!("SPI error deleting network: {e:?}"))
    .unwrap_or_else(|| error!("No network found with id={network_id}"))
}

/// Runs baseline EPS from the immutable imported INP. Use scenarios for what-if changes.
#[pg_extern]
fn epanet_simulate(network_id: i32) -> i32 {
    let inp_text = base_inp_for_network(network_id);
    run_hydraulic_eps(
        &inp_text,
        network_id,
        None,
        &format!("pg_epanet_{network_id}"),
    )
}

fn quality_analysis_enabled_from_inp(inp_text: &str) -> bool {
    let sections = inp::parse_sections(inp_text);
    let rows = sections.get("OPTIONS").cloned().unwrap_or_default();
    for fields in rows {
        if fields.first().is_some_and(|k| k.eq_ignore_ascii_case("Quality")) {
            let val = fields.get(1).map(|s| s.to_lowercase()).unwrap_or_default();
            return !val.starts_with("none") && val != "0";
        }
    }
    false
}

fn effective_inp_for_run(network_id: i32, run_id: i32) -> String {
    let scenario_id = Spi::get_one::<i32>(&format!(
        "SELECT scenario_id FROM epanet.simulation_runs WHERE id = {run_id}"
    ))
    .ok()
    .flatten();
    match scenario_id {
        Some(sid) => scenario::effective_inp_for_scenario(sid).1,
        None => base_inp_for_network(network_id),
    }
}

/// Runs water quality EPS for an existing hydraulic simulation run.
/// Re-runs the hydraulic solver interleaved with quality routing, then stores
/// per-timestep results in epanet.node_quality_results and epanet.link_quality_results.
/// Returns the same run_id. Requires [OPTIONS] Quality to be other than NONE.
#[pg_extern]
fn epanet_simulate_quality(network_id: i32, run_id: i32) -> i32 {
    use crate::ffi::*;
    use std::ffi::{CStr, c_char};

    let (run_network_id, expected_steps) = Spi::get_two::<i32, i32>(&format!(
        "SELECT network_id, n_steps FROM epanet.simulation_runs WHERE id = {run_id}"
    ))
    .unwrap_or_else(|e| error!("SPI error loading simulation run: {e:?}"));

    let run_network_id = run_network_id
        .unwrap_or_else(|| error!("No simulation run found with id={run_id}"));
    if run_network_id != network_id {
        error!(
            "run_id={run_id} belongs to network_id={run_network_id}, not {network_id}"
        );
    }
    let expected_steps = expected_steps.unwrap_or(0);

    let inp_text = effective_inp_for_run(network_id, run_id);
    if !quality_analysis_enabled_from_inp(&inp_text) {
        error!(
            "Network {network_id} has Quality=NONE (or no Quality option); \
             set [OPTIONS] Quality to CHEMICAL, AGE, or TRACE in the INP"
        );
    }

    let files = temp::TempProjectFiles::new(&format!("pg_epanet_{network_id}_wq_{run_id}"));
    files.write_inp(&inp_text);

    let c_inp = temp::path_to_cstring(&files.inp_path);
    let c_rpt = temp::path_to_cstring(&files.rpt_path);
    let c_out = temp::path_to_cstring(&files.out_path);

    let ph = unsafe {
        let mut ph: EN_Project = std::ptr::null_mut();
        let ec = EN_createproject(&mut ph);
        if ec != 0 {
            error!("EN_createproject failed (code {ec})");
        }
        let ec = EN_open(ph, c_inp.as_ptr(), c_rpt.as_ptr(), c_out.as_ptr());
        if ec != 0 && ec != 200 {
            EN_deleteproject(ph);
            error!(
                "EN_open failed (code {ec}): {}",
                epanet_error_message(ec)
            );
        }
        ph
    };

    let (n_nodes, n_links) = unsafe {
        let mut nn: i32 = 0;
        let mut nl: i32 = 0;
        EN_getcount(ph, EN_NODECOUNT, &mut nn);
        EN_getcount(ph, EN_LINKCOUNT, &mut nl);
        (nn, nl)
    };

    let open_h_ec = unsafe { EN_openH(ph) };
    if open_h_ec >= 100 {
        unsafe {
            EN_close(ph);
            EN_deleteproject(ph);
        }
        error!("EN_openH failed (code {open_h_ec})");
    }
    unsafe { EN_initH(ph, EN_NOSAVE) };

    let open_q_ec = unsafe { EN_openQ(ph) };
    if open_q_ec >= 100 {
        unsafe {
            EN_closeH(ph);
            EN_close(ph);
            EN_deleteproject(ph);
        }
        error!("EN_openQ failed (code {open_q_ec})");
    }
    unsafe { EN_initQ(ph, EN_NOSAVE) };

    let mut node_parts: Vec<String> = Vec::new();
    let mut link_parts: Vec<String> = Vec::new();
    let mut n_steps: i32 = 0;

    loop {
        let mut current_time: i64 = 0;
        let ec_h = unsafe { EN_runH(ph, &mut current_time) };
        if ec_h >= 100 {
            unsafe {
                EN_closeQ(ph);
                EN_closeH(ph);
                EN_close(ph);
                EN_deleteproject(ph);
            }
            error!("EN_runH failed (code {ec_h}) at t={current_time}s");
        }
        if (1..100).contains(&ec_h) {
            warning!(
                "epanet: hydraulic warning {} at t={}s: {}",
                ec_h,
                current_time,
                epanet_error_message(ec_h)
            );
        }

        let ec_q = unsafe { EN_runQ(ph, &mut current_time) };
        if ec_q >= 100 {
            unsafe {
                EN_closeQ(ph);
                EN_closeH(ph);
                EN_close(ph);
                EN_deleteproject(ph);
            }
            error!("EN_runQ failed (code {ec_q}) at t={current_time}s");
        }
        if (1..100).contains(&ec_q) {
            warning!(
                "epanet: quality warning {} at t={}s: {}",
                ec_q,
                current_time,
                epanet_error_message(ec_q)
            );
        }

        let step = n_steps;

        unsafe {
            let mut buf = vec![0 as c_char; 64];
            for i in 1..=n_nodes {
                EN_getnodeid(ph, i, buf.as_mut_ptr());
                let mut quality: f64 = 0.0;
                EN_getnodevalue(ph, i, EN_QUALITY, &mut quality);
                let name = CStr::from_ptr(buf.as_ptr())
                    .to_string_lossy()
                    .replace('\'', "''");
                node_parts.push(format!("{step},'{name}',{}", format_epanet_f64(quality)));
            }
        }

        unsafe {
            let mut buf = vec![0 as c_char; 64];
            for i in 1..=n_links {
                EN_getlinkid(ph, i, buf.as_mut_ptr());
                let mut quality: f64 = 0.0;
                EN_getlinkvalue(ph, i, EN_LINKQUAL, &mut quality);
                let name = CStr::from_ptr(buf.as_ptr())
                    .to_string_lossy()
                    .replace('\'', "''");
                link_parts.push(format!("{step},'{name}',{}", format_epanet_f64(quality)));
            }
        }

        n_steps += 1;

        let mut tstep_h: i64 = 0;
        unsafe { EN_nextH(ph, &mut tstep_h) };
        let mut _tstep_q: i64 = 0;
        unsafe { EN_nextQ(ph, &mut _tstep_q) };
        if tstep_h <= 0 {
            break;
        }
    }

    unsafe {
        EN_closeQ(ph);
        EN_closeH(ph);
        EN_close(ph);
        EN_deleteproject(ph);
    }

    if expected_steps > 0 && n_steps != expected_steps {
        warning!(
            "epanet: quality run produced {n_steps} steps, hydraulic run had {expected_steps}"
        );
    }

    Spi::run(&format!(
        "DELETE FROM epanet.node_quality_results WHERE run_id = {run_id}"
    ))
    .ok();
    Spi::run(&format!(
        "DELETE FROM epanet.link_quality_results WHERE run_id = {run_id}"
    ))
    .ok();

    if !node_parts.is_empty() {
        let vals: String = node_parts
            .iter()
            .map(|r| format!("({run_id},{r})"))
            .collect::<Vec<_>>()
            .join(",");
        Spi::run(&format!(
            "INSERT INTO epanet.node_quality_results(run_id,step,node_id,quality) \
             VALUES {vals}"
        ))
        .unwrap_or_else(|e| error!("SPI error inserting node_quality_results: {e:?}"));
    }

    if !link_parts.is_empty() {
        let vals: String = link_parts
            .iter()
            .map(|r| format!("({run_id},{r})"))
            .collect::<Vec<_>>()
            .join(",");
        Spi::run(&format!(
            "INSERT INTO epanet.link_quality_results(run_id,step,link_id,quality) \
             VALUES {vals}"
        ))
        .unwrap_or_else(|e| error!("SPI error inserting link_quality_results: {e:?}"));
    }

    run_id
}

/// Returns the number of nodes whose minimum quality across all timesteps is below `threshold`.
#[pg_extern]
fn epanet_count_nodes_below_threshold(run_id: i32, threshold: f64) -> i64 {
    Spi::get_one::<i64>(&format!(
        "SELECT count(*)::bigint FROM epanet.node_quality_envelope \
         WHERE run_id = {run_id} AND min_quality < {threshold}"
    ))
    .unwrap_or_else(|e| error!("SPI error counting nodes below threshold: {e:?}"))
    .unwrap_or(0)
}

/// Returns rows from the [JUNCTIONS] section of an INP file.
/// demand defaults to 0.0 when absent; pattern is NULL when absent.
#[pg_extern]
fn epanet_junctions(
    inp_text: &str,
) -> TableIterator<
    'static,
    (
        name!(name, String),
        name!(elevation, f64),
        name!(demand, f64),
        name!(pattern, Option<String>),
    ),
>
{
    let mut sections = inp::parse_sections(inp_text);
    let rows = sections.remove("JUNCTIONS").unwrap_or_default();

    TableIterator::new(rows.into_iter().filter_map(|fields| {
        if fields.len() < 2 {
            return None;
        }
        let name = fields[0].clone();
        let elevation: f64 = fields[1].parse().ok()?;
        let demand: f64 = fields.get(2).and_then(|s| s.parse().ok()).unwrap_or(0.0);
        let pattern = fields.get(3).cloned();
        Some((name, elevation, demand, pattern))
    }))
}

/// Returns rows from the [PIPES] section of an INP file.
/// minor_loss defaults to 0.0 when absent; status defaults to 'OPEN' when absent.
/// 'CV' in status indicates a check valve.
#[pg_extern]
fn epanet_pipes(
    inp_text: &str,
) -> TableIterator<
    'static,
    (
        name!(name, String),
        name!(node1, String),
        name!(node2, String),
        name!(length, f64),
        name!(diameter, f64),
        name!(roughness, f64),
        name!(minor_loss, f64),
        name!(status, String),
    ),
>
{
    let mut sections = inp::parse_sections(inp_text);
    let rows = sections.remove("PIPES").unwrap_or_default();

    TableIterator::new(rows.into_iter().filter_map(|fields| {
        if fields.len() < 6 {
            return None;
        }
        let name = fields[0].clone();
        let node1 = fields[1].clone();
        let node2 = fields[2].clone();
        let length: f64 = fields[3].parse().ok()?;
        let diameter: f64 = fields[4].parse().ok()?;
        let roughness: f64 = fields[5].parse().ok()?;
        let minor_loss: f64 = fields.get(6).and_then(|s| s.parse().ok()).unwrap_or(0.0);
        let status = fields.get(7).map(|s| s.to_uppercase()).unwrap_or_else(|| "OPEN".to_string());
        Some((name, node1, node2, length, diameter, roughness, minor_loss, status))
    }))
}

/// Returns X, Y coordinates for nodes from the [COORDINATES] section of an INP file.
#[pg_extern]
fn epanet_coordinates(
    inp_text: &str,
) -> TableIterator<'static, (name!(node_id, String), name!(x, f64), name!(y, f64))>
{
    let mut sections = inp::parse_sections(inp_text);
    let rows = sections.remove("COORDINATES").unwrap_or_default();

    TableIterator::new(rows.into_iter().filter_map(|fields| {
        if fields.len() < 3 {
            return None;
        }
        let node_id = fields[0].clone();
        let x: f64 = fields[1].parse().ok()?;
        let y: f64 = fields[2].parse().ok()?;
        Some((node_id, x, y))
    }))
}

/// Returns intermediate pipe bend vertices from the [VERTICES] section of an INP file.
/// Multiple rows per pipe are allowed.
#[pg_extern]
fn epanet_vertices(
    inp_text: &str,
) -> TableIterator<'static, (name!(link_id, String), name!(x, f64), name!(y, f64))>
{
    let mut sections = inp::parse_sections(inp_text);
    let rows = sections.remove("VERTICES").unwrap_or_default();

    TableIterator::new(rows.into_iter().filter_map(|fields| {
        if fields.len() != 3 {
            return None;
        }
        let link_id = fields[0].clone();
        let x: f64 = fields[1].parse().ok()?;
        let y: f64 = fields[2].parse().ok()?;
        Some((link_id, x, y))
    }))
}

/// Returns rows from the [PUMPS] section of an INP file.
/// Fields from index 3 onward are keyword-value pairs in any order.
/// pump_type is 'HEAD' or 'POWER'; speed is NULL when absent (EPANET defaults to 1.0).
#[pg_extern]
fn epanet_pumps(
    inp_text: &str,
) -> TableIterator<
    'static,
    (
        name!(name, String),
        name!(node1, String),
        name!(node2, String),
        name!(pump_type, Option<String>),
        name!(head_curve, Option<String>),
        name!(power, Option<f64>),
        name!(speed, Option<f64>),
        name!(pattern, Option<String>),
    ),
>
{
    let mut sections = inp::parse_sections(inp_text);
    let rows = sections.remove("PUMPS").unwrap_or_default();

    TableIterator::new(rows.into_iter().filter_map(|fields| {
        if fields.len() < 3 {
            return None;
        }
        let name = fields[0].clone();
        let node1 = fields[1].clone();
        let node2 = fields[2].clone();

        let mut pump_type: Option<String> = None;
        let mut head_curve: Option<String> = None;
        let mut power: Option<f64> = None;
        let mut speed: Option<f64> = None;
        let mut pattern: Option<String> = None;

        let mut i = 3;
        while i + 1 < fields.len() {
            match fields[i].to_uppercase().as_str() {
                "HEAD" => {
                    pump_type = Some("HEAD".to_string());
                    head_curve = Some(fields[i + 1].clone());
                }
                "POWER" => {
                    pump_type = Some("POWER".to_string());
                    power = fields[i + 1].parse().ok();
                }
                "SPEED" => {
                    speed = fields[i + 1].parse().ok();
                }
                "PATTERN" => {
                    pattern = Some(fields[i + 1].clone());
                }
                _ => {}
            }
            i += 2;
        }

        Some((name, node1, node2, pump_type, head_curve, power, speed, pattern))
    }))
}

/// Returns rows from the [VALVES] section of an INP file.
/// setting is TEXT because GPV valves store a curve name there instead of a numeric value.
/// minor_loss defaults to 0.0 when absent.
#[pg_extern]
fn epanet_valves(
    inp_text: &str,
) -> TableIterator<
    'static,
    (
        name!(name, String),
        name!(node1, String),
        name!(node2, String),
        name!(diameter, f64),
        name!(valve_type, String),
        name!(setting, String),
        name!(minor_loss, f64),
    ),
>
{
    let mut sections = inp::parse_sections(inp_text);
    let rows = sections.remove("VALVES").unwrap_or_default();

    TableIterator::new(rows.into_iter().filter_map(|fields| {
        if fields.len() < 6 {
            return None;
        }
        let name = fields[0].clone();
        let node1 = fields[1].clone();
        let node2 = fields[2].clone();
        let diameter: f64 = fields[3].parse().ok()?;
        let valve_type = fields[4].to_uppercase();
        let setting = fields[5].clone();
        let minor_loss: f64 = fields.get(6).and_then(|s| s.parse().ok()).unwrap_or(0.0);
        Some((name, node1, node2, diameter, valve_type, setting, minor_loss))
    }))
}

extension_sql!(
    r#"
CREATE OR REPLACE FUNCTION epanet.effective_node_xy(p_scenario_id int, p_node_id text)
RETURNS TABLE(x float8, y float8)
LANGUAGE sql STABLE AS $$
    SELECT COALESCE(se.coord_x, c.x) AS x, COALESCE(se.coord_y, c.y) AS y
    FROM epanet.scenarios s
    LEFT JOIN epanet.scenario_elements se
      ON se.scenario_id = s.id AND se.name = p_node_id
     AND se.element_type IN ('junction', 'reservoir', 'tank')
    LEFT JOIN epanet.coordinates c
      ON c.network_id = s.network_id AND c.node_id = p_node_id
    WHERE s.id = p_scenario_id
      AND COALESCE(se.coord_x, c.x) IS NOT NULL
$$;

CREATE OR REPLACE FUNCTION epanet_scenario_nodes(p_scenario_id int)
RETURNS TABLE(
    node_id text,
    node_type text,
    provisional boolean,
    geom geometry
)
LANGUAGE sql STABLE AS $$
    WITH s AS (SELECT network_id FROM epanet.scenarios WHERE id = p_scenario_id)
    SELECT n.node_id, n.node_type, false AS provisional, n.geom
    FROM epanet.nodes n, s
    WHERE n.network_id = s.network_id
      AND NOT EXISTS (
          SELECT 1 FROM epanet.scenario_elements se
          WHERE se.scenario_id = p_scenario_id AND se.name = n.node_id
            AND se.element_type IN ('junction', 'reservoir', 'tank')
      )
    UNION ALL
    SELECT se.name,
           se.element_type,
           true,
           COALESCE(se.geom, ST_SetSRID(ST_MakePoint(se.coord_x, se.coord_y), net.srid))
    FROM epanet.scenario_elements se
    JOIN epanet.scenarios sc ON sc.id = se.scenario_id
    JOIN epanet.networks net ON net.id = sc.network_id
    WHERE se.scenario_id = p_scenario_id
      AND se.element_type IN ('junction', 'reservoir', 'tank')
$$;

CREATE OR REPLACE FUNCTION epanet_scenario_links(p_scenario_id int)
RETURNS TABLE(
    link_id text,
    link_type text,
    node1 text,
    node2 text,
    provisional boolean,
    geom geometry
)
LANGUAGE sql STABLE AS $$
    WITH s AS (SELECT network_id FROM epanet.scenarios WHERE id = p_scenario_id)
    SELECT l.link_id, l.link_type, l.node1, l.node2, false, l.geom
    FROM epanet.links l, s
    WHERE l.network_id = s.network_id
      AND NOT EXISTS (
          SELECT 1 FROM epanet.scenario_elements se
          WHERE se.scenario_id = p_scenario_id AND se.name = l.link_id
            AND se.element_type IN ('pipe', 'pump', 'valve')
      )
    UNION ALL
    SELECT se.name,
           se.element_type,
           split_part(se.inp_fields, ' ', 1),
           split_part(se.inp_fields, ' ', 2),
           true,
           se.geom
    FROM epanet.scenario_elements se
    WHERE se.scenario_id = p_scenario_id
      AND se.element_type IN ('pipe', 'pump', 'valve')
$$;

CREATE OR REPLACE FUNCTION epanet_set_node_geom(
    p_network_id int, p_node_id text, p_geom geometry
) RETURNS boolean
LANGUAGE plpgsql AS $$
DECLARE
    v_srid int;
    v_g geometry;
BEGIN
    SELECT srid INTO v_srid FROM epanet.networks WHERE id = p_network_id;
    IF NOT FOUND THEN
        RAISE EXCEPTION 'No network found with id=%', p_network_id;
    END IF;
    v_g := ST_Transform(
        CASE WHEN ST_SRID(p_geom) = 0 THEN ST_SetSRID(p_geom, v_srid) ELSE p_geom END,
        v_srid
    );
    IF ST_GeometryType(v_g) NOT IN ('ST_Point', 'ST_MultiPoint') THEN
        RAISE EXCEPTION 'Expected Point geometry, got %', ST_GeometryType(v_g);
    END IF;
    IF ST_GeometryType(v_g) = 'ST_MultiPoint' THEN
        v_g := ST_GeometryN(v_g, 1);
    END IF;
    RETURN epanet_set_node_coordinates(p_network_id, p_node_id, ST_X(v_g), ST_Y(v_g));
END;
$$;

CREATE OR REPLACE FUNCTION epanet_add_junction_geom(
    p_network_id int, p_name text, p_elevation float8, p_demand float8,
    p_geom geometry, p_pattern text DEFAULT NULL
) RETURNS boolean
LANGUAGE plpgsql AS $$
DECLARE v_g geometry; v_srid int;
BEGIN
    SELECT srid INTO v_srid FROM epanet.networks WHERE id = p_network_id;
    v_g := ST_Transform(
        CASE WHEN ST_SRID(p_geom) = 0 THEN ST_SetSRID(p_geom, v_srid) ELSE p_geom END, v_srid);
    IF ST_GeometryType(v_g) = 'ST_MultiPoint' THEN v_g := ST_GeometryN(v_g, 1); END IF;
    RETURN epanet_add_junction(p_network_id, p_name, p_elevation, p_demand, ST_X(v_g), ST_Y(v_g), p_pattern);
END;
$$;

CREATE OR REPLACE FUNCTION epanet_add_reservoir_geom(
    p_network_id int, p_name text, p_head float8,
    p_geom geometry, p_pattern text DEFAULT NULL
) RETURNS boolean
LANGUAGE plpgsql AS $$
DECLARE v_g geometry; v_srid int;
BEGIN
    SELECT srid INTO v_srid FROM epanet.networks WHERE id = p_network_id;
    v_g := ST_Transform(
        CASE WHEN ST_SRID(p_geom) = 0 THEN ST_SetSRID(p_geom, v_srid) ELSE p_geom END, v_srid);
    IF ST_GeometryType(v_g) = 'ST_MultiPoint' THEN v_g := ST_GeometryN(v_g, 1); END IF;
    RETURN epanet_add_reservoir(p_network_id, p_name, p_head, ST_X(v_g), ST_Y(v_g), p_pattern);
END;
$$;

CREATE OR REPLACE FUNCTION epanet_add_tank_geom(
    p_network_id int, p_name text,
    p_elevation float8, p_init_level float8, p_min_level float8,
    p_max_level float8, p_diameter float8, p_min_volume float8,
    p_geom geometry,
    p_volume_curve text DEFAULT NULL, p_overflow text DEFAULT NULL
) RETURNS boolean
LANGUAGE plpgsql AS $$
DECLARE v_g geometry; v_srid int;
BEGIN
    SELECT srid INTO v_srid FROM epanet.networks WHERE id = p_network_id;
    v_g := ST_Transform(
        CASE WHEN ST_SRID(p_geom) = 0 THEN ST_SetSRID(p_geom, v_srid) ELSE p_geom END, v_srid);
    IF ST_GeometryType(v_g) = 'ST_MultiPoint' THEN v_g := ST_GeometryN(v_g, 1); END IF;
    RETURN epanet_add_tank(p_network_id, p_name, p_elevation, p_init_level, p_min_level,
        p_max_level, p_diameter, p_min_volume, ST_X(v_g), ST_Y(v_g), p_volume_curve, p_overflow);
END;
$$;

CREATE OR REPLACE FUNCTION epanet_add_pipe_geom(
    p_network_id int, p_name text, p_node1 text, p_node2 text,
    p_geom geometry,
    p_length float8, p_diameter float8, p_roughness float8,
    p_minor_loss float8 DEFAULT 0, p_status text DEFAULT 'Open'
) RETURNS boolean
LANGUAGE plpgsql AS $$
DECLARE v_g geometry; v_srid int;
BEGIN
    SELECT srid INTO v_srid FROM epanet.networks WHERE id = p_network_id;
    v_g := ST_Transform(
        CASE WHEN ST_SRID(p_geom) = 0 THEN ST_SetSRID(p_geom, v_srid) ELSE p_geom END, v_srid);
    IF ST_GeometryType(v_g) NOT IN ('ST_LineString', 'ST_MultiLineString') THEN
        RAISE EXCEPTION 'Expected LineString geometry for pipe, got %', ST_GeometryType(v_g);
    END IF;
    IF ST_GeometryType(v_g) = 'ST_MultiLineString' THEN
        v_g := ST_GeometryN(v_g, 1);
    END IF;
    PERFORM epanet_add_pipe(p_network_id, p_name, p_node1, p_node2,
        p_length, p_diameter, p_roughness, p_minor_loss, p_status);
    PERFORM epanet_apply_pipe_shape(p_network_id, p_name, ST_AsText(v_g));
    RETURN true;
END;
$$;

CREATE OR REPLACE FUNCTION epanet_set_scenario_node_geom(
    p_scenario_id int, p_node_id text, p_geom geometry
) RETURNS boolean
LANGUAGE plpgsql AS $$
DECLARE v_g geometry; v_srid int;
BEGIN
    SELECT n.srid INTO v_srid
    FROM epanet.scenarios s JOIN epanet.networks n ON n.id = s.network_id
    WHERE s.id = p_scenario_id;
    IF NOT FOUND THEN
        RAISE EXCEPTION 'No scenario found with id=%', p_scenario_id;
    END IF;
    v_g := ST_Transform(
        CASE WHEN ST_SRID(p_geom) = 0 THEN ST_SetSRID(p_geom, v_srid) ELSE p_geom END, v_srid);
    IF ST_GeometryType(v_g) = 'ST_MultiPoint' THEN v_g := ST_GeometryN(v_g, 1); END IF;
    RETURN epanet_set_scenario_node_coordinates(p_scenario_id, p_node_id, ST_X(v_g), ST_Y(v_g));
END;
$$;
"#,
    name = "map_geom_sql",
    requires = [
        epanet_set_node_coordinates,
        epanet_add_junction,
        epanet_add_reservoir,
        epanet_add_tank,
        epanet_add_pipe,
        epanet_apply_pipe_shape,
        epanet_set_scenario_node_coordinates,
    ],
);

#[cfg(any(test, feature = "pg_test"))]
#[pg_schema]
mod tests {
    use pgrx::prelude::*;

    #[pg_test]
    fn test_hello_pg_epanet() {
        assert_eq!("Hello, pg_epanet", crate::hello_pg_epanet());
    }

    #[pg_test]
    fn test_epanet_import_returns_network_id_and_creates_tables() {
        let inp = "$inp$
[JUNCTIONS]
J1  100.0  10.0  PD1
J2  200.0
[RESERVOIRS]
R1  300.0
[PIPES]
P1  J1  J2  100.0  200.0  0.01
[COORDINATES]
J1  100.0  200.0
J2  150.0  250.0
R1  50.0   100.0
[VERTICES]
P1  120.0  220.0
$inp$";

        let nid = Spi::get_one::<i32>(
            &format!("SELECT epanet_import('test_red', {inp}, 32632)"),
        )
        .unwrap()
        .unwrap();
        assert!(nid > 0);

        // Catalogue tables must contain the imported data
        let n_j = Spi::get_one::<i64>(
            &format!("SELECT count(*)::bigint FROM epanet.junctions WHERE network_id = {nid}"),
        ).unwrap().unwrap();
        assert_eq!(n_j, 2);

        let n_p = Spi::get_one::<i64>(
            &format!("SELECT count(*)::bigint FROM epanet.pipes WHERE network_id = {nid}"),
        ).unwrap().unwrap();
        assert_eq!(n_p, 1);

        // Nodes must have geometry generated
        let geom_notnull = Spi::get_one::<i64>(&format!(
            "SELECT count(*)::bigint FROM epanet.junctions \
             WHERE network_id = {nid} AND geom IS NOT NULL"
        )).unwrap().unwrap();
        assert_eq!(geom_notnull, 2);

        // The pipe must have a LineString geometry
        let pipe_geom = Spi::get_one::<i64>(&format!(
            "SELECT count(*)::bigint FROM epanet.pipes \
             WHERE network_id = {nid} AND geom IS NOT NULL"
        )).unwrap().unwrap();
        assert_eq!(pipe_geom, 1);

        // The original INP text must be stored
        let stored_len = Spi::get_one::<i32>(&format!(
            "SELECT length(inp_text) FROM epanet.networks WHERE id = {nid}"
        )).unwrap().unwrap();
        assert!(stored_len > 0);
    }

    #[pg_test]
    fn test_epanet_import_accumulates_versions() {
        let inp = "'[JUNCTIONS]\nJ1  50.0\n[COORDINATES]\nJ1  10.0  20.0\n'";
        let nid1 = Spi::get_one::<i32>(
            &format!("SELECT epanet_import('test_acum', {inp})"),
        ).unwrap().unwrap();
        let nid2 = Spi::get_one::<i32>(
            &format!("SELECT epanet_import('test_acum', {inp})"),
        ).unwrap().unwrap();

        // Each call must produce a distinct network_id
        assert_ne!(nid1, nid2);
        // Both versions must coexist
        let n = Spi::get_one::<i64>(
            "SELECT count(*)::bigint FROM epanet.networks WHERE name = 'test_acum'",
        ).unwrap().unwrap();
        assert_eq!(n, 2);
    }

    #[pg_test]
    fn test_epanet_delete_removes_network_and_topology() {
        let inp = "'[JUNCTIONS]\nJ1  50.0\n[COORDINATES]\nJ1  10.0  20.0\n'";
        let nid = Spi::get_one::<i32>(
            &format!("SELECT epanet_import('test_del', {inp})"),
        )
        .unwrap()
        .unwrap();

        let deleted = Spi::get_one::<bool>(&format!("SELECT epanet_delete({nid})"))
            .unwrap()
            .unwrap();
        assert!(deleted);

        let n_net = Spi::get_one::<i64>(
            &format!("SELECT count(*)::bigint FROM epanet.networks WHERE id = {nid}"),
        )
        .unwrap()
        .unwrap();
        assert_eq!(n_net, 0);

        let n_j = Spi::get_one::<i64>(
            &format!("SELECT count(*)::bigint FROM epanet.junctions WHERE network_id = {nid}"),
        )
        .unwrap()
        .unwrap();
        assert_eq!(n_j, 0);
    }

    #[pg_test]
    fn test_epanet_junctions_have_gist_index() {
        let n = Spi::get_one::<i64>(
            "SELECT count(*)::bigint FROM pg_indexes \
             WHERE schemaname = 'epanet' AND tablename = 'junctions' AND indexname = 'junctions_geom'",
        )
        .unwrap()
        .unwrap();
        assert_eq!(n, 1);
    }

    #[pg_test]
    fn test_epanet_topology_endpoint_indexes() {
        for (table, index) in [
            ("pipes", "pipes_node1"),
            ("pipes", "pipes_node2"),
            ("pumps", "pumps_node1"),
            ("valves", "valves_node1"),
            ("simulation_runs", "simulation_runs_network"),
        ] {
            let n = Spi::get_one::<i64>(&format!(
                "SELECT count(*)::bigint FROM pg_indexes \
                 WHERE schemaname = 'epanet' AND tablename = '{table}' AND indexname = '{index}'"
            ))
            .unwrap()
            .unwrap();
            assert_eq!(n, 1, "missing index {index} on {table}");
        }
    }

    #[pg_test]
    fn test_epanet_junctions_demand_defaults_to_zero() {
        let (demand, pattern) = Spi::get_two::<f64, String>(
            "SELECT demand, pattern FROM epanet_junctions($inp$
[JUNCTIONS]
J1  100.0
$inp$)",
        )
        .unwrap();
        assert_eq!(demand.unwrap(), 0.0);
        assert!(pattern.is_none());
    }

    #[pg_test]
    fn test_epanet_junctions_with_demand_and_pattern() {
        let (demand, pattern) = Spi::get_two::<f64, String>(
            "SELECT demand, pattern FROM epanet_junctions($inp$
[JUNCTIONS]
J1  100.0  50.0  PD1
$inp$) WHERE name = 'J1'",
        )
        .unwrap();
        assert_eq!(demand.unwrap(), 50.0);
        assert_eq!(pattern.unwrap().as_str(), "PD1");
    }

    #[pg_test]
    fn test_epanet_pipes_minimum_fields() {
        let (minor_loss, status) = Spi::get_two::<f64, String>(
            "SELECT minor_loss, status FROM epanet_pipes($inp$
[PIPES]
P1  J1  J2  100.0  200.0  0.01
$inp$)",
        )
        .unwrap();
        assert_eq!(minor_loss.unwrap(), 0.0);
        assert_eq!(status.unwrap().as_str(), "OPEN");
    }

    #[pg_test]
    fn test_epanet_pipes_all_fields() {
        let inp = "$inp$
[PIPES]
P1  J1  J2  500.0  300.0  0.05  1.5  CLOSED
$inp$";
        let (length, roughness) = Spi::get_two::<f64, f64>(
            &format!("SELECT length, roughness FROM epanet_pipes({inp}) WHERE name = 'P1'"),
        )
        .unwrap();
        assert_eq!(length.unwrap(), 500.0);
        assert_eq!(roughness.unwrap(), 0.05);

        let (minor_loss, status) = Spi::get_two::<f64, String>(
            &format!("SELECT minor_loss, status FROM epanet_pipes({inp}) WHERE name = 'P1'"),
        )
        .unwrap();
        assert_eq!(minor_loss.unwrap(), 1.5);
        assert_eq!(status.unwrap().as_str(), "CLOSED");
    }

    #[pg_test]
    fn test_epanet_pipes_cv() {
        let status = Spi::get_one::<String>(
            "SELECT status FROM epanet_pipes($inp$
[PIPES]
P1  J1  J2  100.0  200.0  0.01  0.0  CV
$inp$)",
        )
        .unwrap()
        .unwrap();
        assert_eq!(status, "CV");
    }

    #[pg_test]
    fn test_epanet_pipes_status_uppercased() {
        let status = Spi::get_one::<String>(
            "SELECT status FROM epanet_pipes($inp$
[PIPES]
P1  J1  J2  100.0  200.0  0.01  0.0  closed
$inp$)",
        )
        .unwrap()
        .unwrap();
        assert_eq!(status, "CLOSED");
    }

    #[pg_test]
    fn test_epanet_coordinates() {
        let (x, y) = Spi::get_two::<f64, f64>(
            "SELECT x, y FROM epanet_coordinates($inp$
[COORDINATES]
J1  100.5  200.75
$inp$) WHERE node_id = 'J1'",
        )
        .unwrap();
        assert_eq!(x.unwrap(), 100.5);
        assert_eq!(y.unwrap(), 200.75);
    }

    #[pg_test]
    fn test_epanet_vertices_multiple_per_pipe() {
        let n = Spi::get_one::<i64>(
            "SELECT count(*)::bigint FROM epanet_vertices($inp$
[VERTICES]
P1  10.0  20.0
P1  15.0  25.0
P2  50.0  60.0
$inp$) WHERE link_id = 'P1'",
        )
        .unwrap()
        .unwrap();
        assert_eq!(n, 2);
    }

    #[pg_test]
    fn test_epanet_reservoirs_row_count() {
        let n = Spi::get_one::<i64>(
            "SELECT count(*)::bigint FROM epanet_reservoirs($inp$
[RESERVOIRS]
;ID    Head    Pattern
R1     100.0
R2     200.0   pat1
$inp$)",
        )
        .unwrap()
        .unwrap();
        assert_eq!(n, 2);
    }

    #[pg_test]
    fn test_epanet_reservoirs_values() {
        let head = Spi::get_one::<f64>(
            "SELECT head FROM epanet_reservoirs($inp$
[RESERVOIRS]
R1  150.5
$inp$) WHERE name = 'R1'",
        )
        .unwrap()
        .unwrap();
        assert_eq!(head, 150.5);
    }

    #[pg_test]
    fn test_epanet_reservoirs_pattern_null_when_absent() {
        let pattern = Spi::get_one::<String>(
            "SELECT pattern FROM epanet_reservoirs($inp$
[RESERVOIRS]
R1  100.0
$inp$)",
        )
        .unwrap();
        // pattern must be NULL when absent
        assert!(pattern.is_none());
    }

    #[pg_test]
    fn test_epanet_tanks_row_count() {
        let n = Spi::get_one::<i64>(
            "SELECT count(*)::bigint FROM epanet_tanks($inp$
[TANKS]
;ID   Elev  InitLvl MinLvl MaxLvl Diam
T1    50.0  10.0    2.0    20.0   15.0
T2    60.0  5.0     1.0    18.0   10.0   100.0   C1
$inp$)",
        )
        .unwrap()
        .unwrap();
        assert_eq!(n, 2);
    }

    #[pg_test]
    fn test_epanet_tanks_min_volume_defaults_to_zero() {
        let min_vol = Spi::get_one::<f64>(
            "SELECT min_volume FROM epanet_tanks($inp$
[TANKS]
T1  50.0  10.0  2.0  20.0  15.0
$inp$)",
        )
        .unwrap()
        .unwrap();
        assert_eq!(min_vol, 0.0);
    }

    #[pg_test]
    fn test_epanet_tanks_volume_curve_name() {
        let curve = Spi::get_one::<String>(
            "SELECT volume_curve FROM epanet_tanks($inp$
[TANKS]
T1  50.0  10.0  2.0  20.0  15.0  100.0  MiCurva
$inp$) WHERE name = 'T1'",
        )
        .unwrap()
        .unwrap();
        assert_eq!(curve, "MiCurva");
    }

    #[pg_test]
    fn test_epanet_tanks_asterisk_is_null() {
        let curve = Spi::get_one::<String>(
            "SELECT volume_curve FROM epanet_tanks($inp$
[TANKS]
T1  50.0  10.0  2.0  20.0  15.0  0.0  *
$inp$)",
        )
        .unwrap();
        assert!(curve.is_none());
    }

    #[pg_test]
    fn test_epanet_missing_section_returns_empty() {
        let n = Spi::get_one::<i64>(
            "SELECT count(*)::bigint FROM epanet_tanks('[JUNCTIONS]
J1  10.0
')",
        )
        .unwrap()
        .unwrap();
        assert_eq!(n, 0);
    }

    #[pg_test]
    fn test_epanet_pumps_head() {
        let (pump_type, head_curve, power) = Spi::get_three::<String, String, f64>(
            "SELECT pump_type, head_curve, power FROM epanet_pumps($inp$
[PUMPS]
P1  J1  J2  HEAD  HC1
$inp$) WHERE name = 'P1'",
        )
        .unwrap();
        assert_eq!(pump_type.unwrap().as_str(), "HEAD");
        assert_eq!(head_curve.unwrap().as_str(), "HC1");
        assert!(power.is_none());
    }

    #[pg_test]
    fn test_epanet_pumps_power_with_speed_and_pattern() {
        let inp = "$inp$
[PUMPS]
P2  J3  J4  POWER  10.5  SPEED  1.5  PATTERN  pat1
$inp$";
        let (pump_type, power) = Spi::get_two::<String, f64>(
            &format!("SELECT pump_type, power FROM epanet_pumps({inp}) WHERE name = 'P2'"),
        )
        .unwrap();
        assert_eq!(pump_type.unwrap().as_str(), "POWER");
        assert_eq!(power.unwrap(), 10.5);

        let (speed, pattern) = Spi::get_two::<f64, String>(
            &format!("SELECT speed, pattern FROM epanet_pumps({inp}) WHERE name = 'P2'"),
        )
        .unwrap();
        assert_eq!(speed.unwrap(), 1.5);
        assert_eq!(pattern.unwrap().as_str(), "pat1");
    }

    #[pg_test]
    fn test_epanet_pumps_speed_null_when_absent() {
        let speed = Spi::get_one::<f64>(
            "SELECT speed FROM epanet_pumps($inp$
[PUMPS]
P1  J1  J2  HEAD  HC1
$inp$)",
        )
        .unwrap();
        assert!(speed.is_none());
    }

    #[pg_test]
    fn test_epanet_valves_prv() {
        let (valve_type, setting, minor_loss) = Spi::get_three::<String, String, f64>(
            "SELECT valve_type, setting, minor_loss FROM epanet_valves($inp$
[VALVES]
V1  J1  J2  12.0  PRV  100.0  0.5
$inp$) WHERE name = 'V1'",
        )
        .unwrap();
        assert_eq!(valve_type.unwrap().as_str(), "PRV");
        assert_eq!(setting.unwrap().as_str(), "100.0");
        assert_eq!(minor_loss.unwrap(), 0.5);
    }

    #[pg_test]
    fn test_epanet_valves_gpv_setting_is_curve_name() {
        let setting = Spi::get_one::<String>(
            "SELECT setting FROM epanet_valves($inp$
[VALVES]
V2  J3  J4  10.0  GPV  GC1
$inp$) WHERE name = 'V2'",
        )
        .unwrap()
        .unwrap();
        assert_eq!(setting, "GC1");
    }

    #[pg_test]
    fn test_epanet_valves_minor_loss_defaults_to_zero() {
        let minor_loss = Spi::get_one::<f64>(
            "SELECT minor_loss FROM epanet_valves($inp$
[VALVES]
V3  J5  J6  8.0  TCV  0.3
$inp$)",
        )
        .unwrap()
        .unwrap();
        assert_eq!(minor_loss, 0.0);
    }

    #[pg_test]
    fn test_epanet_valves_type_uppercased() {
        let valve_type = Spi::get_one::<String>(
            "SELECT valve_type FROM epanet_valves($inp$
[VALVES]
V1  J1  J2  10.0  fcv  50.0
$inp$)",
        )
        .unwrap()
        .unwrap();
        assert_eq!(valve_type, "FCV");
    }

    const META_INP: &str = "$inp$
[PATTERNS]
PD1 1.0 1.2 1.4
[CURVES]
C1 0.0 10.0
C1 1.0 8.0
[OPTIONS]
Units LPS
Demand Multiplier 1.5
[TIMES]
Duration 24:00
[CONTROLS]
LINK P1 CLOSED IF NODE J1 ABOVE 100
[RULES]
RULE R1
IF TANK T1 LEVEL > 8
THEN PUMP PU1 STATUS = CLOSED
PRIORITY 1
[DEMANDS]
J1 5.0 PD1
[EMITTERS]
J2 0.25
[STATUS]
P1 Open
[SOURCES]
J1 CONCEN 0.5 PD1
[REACTIONS]
Order Bulk 1
[QUALITY]
Tolerance 0.01
[ENERGY]
Global Efficiency 75
[REPORT]
Status No
$inp$";

    #[pg_test]
    fn test_epanet_patterns_table_fn() {
        let n = Spi::get_one::<i64>(&format!(
            "SELECT count(*)::bigint FROM epanet_patterns({META_INP})"
        ))
        .unwrap()
        .unwrap();
        assert_eq!(n, 3);
    }

    #[pg_test]
    fn test_epanet_curves_table_fn() {
        let n = Spi::get_one::<i64>(&format!(
            "SELECT count(*)::bigint FROM epanet_curves({META_INP})"
        ))
        .unwrap()
        .unwrap();
        assert_eq!(n, 2);
    }

    #[pg_test]
    fn test_epanet_options_two_word_key() {
        let val = Spi::get_one::<String>(&format!(
            "SELECT value FROM epanet_options({META_INP}) WHERE key = 'Demand Multiplier'"
        ))
        .unwrap()
        .unwrap();
        assert_eq!(val, "1.5");
    }

    #[pg_test]
    fn test_epanet_rules_table_fn() {
        let n = Spi::get_one::<i64>(&format!(
            "SELECT count(*)::bigint FROM epanet_rules({META_INP})"
        ))
        .unwrap()
        .unwrap();
        assert_eq!(n, 1);
    }

    #[pg_test]
    fn test_epanet_import_metadata_sections() {
        let inp = r#"[JUNCTIONS]
J1 50.0
[COORDINATES]
J1 1.0 2.0
[PATTERNS]
PD1 1.0 1.2
[CURVES]
C1 0.0 10.0
[OPTIONS]
Units LPS
[RULES]
RULE R1
IF TANK T1 LEVEL > 8
THEN PUMP PU1 STATUS = CLOSED
PRIORITY 1
"#;
        let nid = Spi::get_one::<i32>(&format!(
            "SELECT epanet_import('meta_test', $${inp}$$, 4326)"
        ))
        .unwrap()
        .unwrap();

        let patterns = Spi::get_one::<i64>(&format!(
            "SELECT count(*)::bigint FROM epanet.patterns WHERE network_id = {nid}"
        ))
        .unwrap()
        .unwrap();
        assert_eq!(patterns, 2);

        let rules = Spi::get_one::<i64>(&format!(
            "SELECT count(*)::bigint FROM epanet.rules WHERE network_id = {nid}"
        ))
        .unwrap()
        .unwrap();
        assert_eq!(rules, 1);

        let _ = Spi::get_one::<bool>(&format!("SELECT epanet_delete({nid})")).unwrap();
    }

    #[pg_test]
    fn test_quality_results_schema() {
        for (table, index) in [
            ("node_quality_results", "node_quality_results_run"),
            ("node_quality_results", "node_quality_results_run_step"),
            ("link_quality_results", "link_quality_results_run"),
            ("link_quality_results", "link_quality_results_run_step"),
        ] {
            let n = Spi::get_one::<i64>(&format!(
                "SELECT count(*)::bigint FROM pg_indexes \
                 WHERE schemaname = 'epanet' AND tablename = '{table}' AND indexname = '{index}'"
            ))
            .unwrap()
            .unwrap();
            assert_eq!(n, 1, "missing index {index} on {table}");
        }

        let view_exists = Spi::get_one::<i64>(
            "SELECT count(*)::bigint FROM pg_views \
             WHERE schemaname = 'epanet' AND viewname = 'node_quality_envelope'",
        )
        .unwrap()
        .unwrap();
        assert_eq!(view_exists, 1);
    }

    #[pg_test]
    fn test_epanet_simulate_quality() {
        let inp = r#"[JUNCTIONS]
J1 100.0 10.0
J2 90.0 0.0
[RESERVOIRS]
R1 150.0
[PIPES]
P1 J1 J2 100.0 200.0 100.0
[COORDINATES]
J1 0.0 0.0
J2 100.0 0.0
R1 0.0 100.0
[QUALITY]
J1 1.0
R1 1.0
[OPTIONS]
Units LPS
Headloss H-W
Quality Chemical mg/L
[TIMES]
Duration 1:00
Hydraulic Timestep 1:00
Report Timestep 1:00
"#;
        let nid = Spi::get_one::<i32>(&format!(
            "SELECT epanet_import('wq_test', $${inp}$$, 4326)"
        ))
        .unwrap()
        .unwrap();

        let run_id = Spi::get_one::<i32>(&format!("SELECT epanet_simulate({nid})"))
            .unwrap()
            .unwrap();
        assert!(run_id > 0);

        let quality_run_id =
            Spi::get_one::<i32>(&format!("SELECT epanet_simulate_quality({nid}, {run_id})"))
                .unwrap()
                .unwrap();
        assert_eq!(quality_run_id, run_id);

        let node_rows = Spi::get_one::<i64>(&format!(
            "SELECT count(*)::bigint FROM epanet.node_quality_results WHERE run_id = {run_id}"
        ))
        .unwrap()
        .unwrap();
        assert!(node_rows > 0);

        let link_rows = Spi::get_one::<i64>(&format!(
            "SELECT count(*)::bigint FROM epanet.link_quality_results WHERE run_id = {run_id}"
        ))
        .unwrap()
        .unwrap();
        assert!(link_rows > 0);

        let below = Spi::get_one::<i64>(&format!(
            "SELECT epanet_count_nodes_below_threshold({run_id}, 9999.0)"
        ))
        .unwrap()
        .unwrap();
        assert!(below >= 0);

        let _ = Spi::get_one::<bool>(&format!("SELECT epanet_delete({nid})")).unwrap();
    }

    #[pg_test]
    fn test_epanet_export_roundtrip() {
        let inp = r#"[JUNCTIONS]
J1 100.0 10.0 PD1
J2 90.0 0.0
[RESERVOIRS]
R1 150.0
[PIPES]
P1 J1 J2 100.0 200.0 100.0
[COORDINATES]
J1 0.0 0.0
J2 100.0 0.0
R1 0.0 100.0
[PATTERNS]
PD1 1.0 1.2
[OPTIONS]
Units LPS
Quality None mg/L
"#;
        let nid = Spi::get_one::<i32>(&format!(
            "SELECT epanet_import('export_test', $${inp}$$, 4326)"
        ))
        .unwrap()
        .unwrap();

        let exported = Spi::get_one::<String>(&format!("SELECT epanet_export({nid})"))
            .unwrap()
            .unwrap();
        assert!(exported.contains("[JUNCTIONS]"));
        assert!(exported.contains("J1"));
        assert!(exported.contains("[END]"));

        let n_j = Spi::get_one::<i64>(&format!(
            "SELECT count(*)::bigint FROM epanet_junctions({})",
            crate::sql_text(&exported)
        ))
        .unwrap()
        .unwrap();
        assert_eq!(n_j, 2);

        let _ = Spi::get_one::<bool>(&format!("SELECT epanet_delete({nid})")).unwrap();
    }

    #[pg_test]
    fn test_epanet_import_replace() {
        let inp1 = "'[JUNCTIONS]\nJ1  50.0\n[COORDINATES]\nJ1  1.0  2.0\n'";
        let inp2 = "'[JUNCTIONS]\nJ2  60.0\n[COORDINATES]\nJ2  3.0  4.0\n'";
        let nid1 = Spi::get_one::<i32>(&format!(
            "SELECT epanet_import('repl_test', {inp1}, 4326, true)"
        ))
        .unwrap()
        .unwrap();
        let nid2 = Spi::get_one::<i32>(&format!(
            "SELECT epanet_import('repl_test', {inp2}, 4326, true)"
        ))
        .unwrap()
        .unwrap();
        assert_ne!(nid1, nid2);

        let n = Spi::get_one::<i64>(
            "SELECT count(*)::bigint FROM epanet.networks WHERE name = 'repl_test'",
        )
        .unwrap()
        .unwrap();
        assert_eq!(n, 1);

        let j = Spi::get_one::<String>(&format!(
            "SELECT name FROM epanet.junctions WHERE network_id = {nid2}"
        ))
        .unwrap()
        .unwrap();
        assert_eq!(j, "J2");

        let _ = Spi::get_one::<bool>(&format!("SELECT epanet_delete({nid2})")).unwrap();
    }

    #[pg_test]
    fn test_epanet_validate_missing_node() {
        let inp = r#"[JUNCTIONS]
J1 100.0
[PIPES]
P1 J1 MISSING 100.0 200.0 100.0
[COORDINATES]
J1 0.0 0.0
"#;
        let nid = Spi::get_one::<i32>(&format!(
            "SELECT epanet_import('val_test', $${inp}$$, 4326)"
        ))
        .unwrap()
        .unwrap();

        let errors = Spi::get_one::<i64>(&format!(
            "SELECT count(*)::bigint FROM epanet_validate({nid}) WHERE severity = 'error'"
        ))
        .unwrap()
        .unwrap();
        assert!(errors >= 1);

        let _ = Spi::get_one::<bool>(&format!("SELECT epanet_delete({nid})")).unwrap();
    }

    #[pg_test]
    fn test_epanet_scenario_demand_override() {
        let inp = r#"[JUNCTIONS]
J1 100.0 10.0
J2 90.0 0.0
[RESERVOIRS]
R1 150.0
[PIPES]
P1 J1 J2 100.0 200.0 100.0
[COORDINATES]
J1 0.0 0.0
J2 100.0 0.0
R1 0.0 100.0
[OPTIONS]
Units LPS
Headloss H-W
[TIMES]
Duration 1:00
Hydraulic Timestep 1:00
Report Timestep 1:00
"#;
        let nid = Spi::get_one::<i32>(&format!(
            "SELECT epanet_import('scn_test', $${inp}$$, 4326)"
        ))
        .unwrap()
        .unwrap();

        let original = Spi::get_one::<String>(&format!(
            "SELECT inp_text FROM epanet.networks WHERE id = {nid}"
        ))
        .unwrap()
        .unwrap();
        assert!(original.contains("10.0"));
        assert!(!original.contains("999"));

        let sid = Spi::get_one::<i32>(&format!(
            "SELECT epanet_create_scenario({nid}, 'high_demand', NULL, 1.0)"
        ))
        .unwrap()
        .unwrap();
        Spi::run(&format!(
            "SELECT epanet_set_scenario_override({sid}, 'junction', 'J1', 'demand', '999')"
        ))
        .unwrap();

        let run_id = Spi::get_one::<i32>(&format!("SELECT epanet_simulate_scenario({sid})"))
            .unwrap()
            .unwrap();
        assert!(run_id > 0);

        let still_original = Spi::get_one::<String>(&format!(
            "SELECT inp_text FROM epanet.networks WHERE id = {nid}"
        ))
        .unwrap()
        .unwrap();
        assert!(!still_original.contains("999"));

        let _ = Spi::get_one::<bool>(&format!("SELECT epanet_delete({nid})")).unwrap();
    }

    #[pg_test]
    fn test_epanet_scenario_pipe_closure() {
        let inp = r#"[JUNCTIONS]
J1 100.0 10.0
J2 90.0 0.0
[RESERVOIRS]
R1 150.0
[PIPES]
P1 J1 J2 100.0 200.0 100.0 0.0 Open
[COORDINATES]
J1 0.0 0.0
J2 100.0 0.0
R1 0.0 100.0
[OPTIONS]
Units LPS
[TIMES]
Duration 1:00
Hydraulic Timestep 1:00
Report Timestep 1:00
"#;
        let nid = Spi::get_one::<i32>(&format!(
            "SELECT epanet_import('closure_test', $${inp}$$, 4326)"
        ))
        .unwrap()
        .unwrap();

        let sid = Spi::get_one::<i32>(&format!(
            "SELECT epanet_scenario_pipe_closure({nid}, 'p1_break', 'P1')"
        ))
        .unwrap()
        .unwrap();

        let n = Spi::get_one::<i64>(&format!(
            "SELECT count(*)::bigint FROM epanet.scenario_overrides \
             WHERE scenario_id = {sid} AND target_id = 'P1'"
        ))
        .unwrap()
        .unwrap();
        assert_eq!(n, 1);

        let _ = Spi::get_one::<bool>(&format!("SELECT epanet_delete({nid})")).unwrap();
    }

    #[pg_test]
    fn test_epanet_compare_runs() {
        let inp = r#"[JUNCTIONS]
J1 100.0 10.0
J2 90.0 0.0
[RESERVOIRS]
R1 150.0
[PIPES]
P1 J1 J2 100.0 200.0 100.0
[COORDINATES]
J1 0.0 0.0
J2 100.0 0.0
R1 0.0 100.0
[OPTIONS]
Units LPS
[TIMES]
Duration 1:00
Hydraulic Timestep 1:00
Report Timestep 1:00
"#;
        let nid = Spi::get_one::<i32>(&format!(
            "SELECT epanet_import('cmp_test', $${inp}$$, 4326)"
        ))
        .unwrap()
        .unwrap();

        let run_a = Spi::get_one::<i32>(&format!("SELECT epanet_simulate({nid})"))
            .unwrap()
            .unwrap();
        let sid = Spi::get_one::<i32>(&format!(
            "SELECT epanet_scenario_pipe_closure({nid}, 'closed', 'P1')"
        ))
        .unwrap()
        .unwrap();
        let run_b = Spi::get_one::<i32>(&format!("SELECT epanet_simulate_scenario({sid})"))
            .unwrap()
            .unwrap();

        let n = Spi::get_one::<i64>(&format!(
            "SELECT count(*)::bigint FROM epanet_compare_runs({run_a}, {run_b})"
        ))
        .unwrap()
        .unwrap();
        assert!(n > 0);

        let _ = Spi::get_one::<bool>(&format!("SELECT epanet_delete({nid})")).unwrap();
    }

    #[pg_test]
    fn test_epanet_add_scenario_junction_and_pipe() {
        let inp = r#"[JUNCTIONS]
J1 100.0 10.0
[RESERVOIRS]
R1 150.0
[PIPES]
P1 J1 R1 100.0 200.0 100.0
[COORDINATES]
J1 0.0 0.0
R1 0.0 100.0
[OPTIONS]
Units LPS
[TIMES]
Duration 1:00
Hydraulic Timestep 1:00
Report Timestep 1:00
"#;
        let nid = Spi::get_one::<i32>(&format!(
            "SELECT epanet_import('topo_scn', $${inp}$$, 4326)"
        ))
        .unwrap()
        .unwrap();

        let sid = Spi::get_one::<i32>(&format!(
            "SELECT epanet_create_scenario({nid}, 'extension', NULL, 1.0)"
        ))
        .unwrap()
        .unwrap();

        Spi::run(&format!(
            "SELECT epanet_add_scenario_junction({sid}, 'J2', 95.0, 5.0, 100.0, 0.0, NULL)"
        ))
        .unwrap();
        Spi::run(&format!(
            "SELECT epanet_add_scenario_pipe({sid}, 'P2', 'J2', 'R1', 50.0, 150.0, 100.0, 0.0, 'Open')"
        ))
        .unwrap();

        let base_j = Spi::get_one::<i64>(&format!(
            "SELECT count(*)::bigint FROM epanet.junctions WHERE network_id = {nid} AND name = 'J2'"
        ))
        .unwrap()
        .unwrap();
        assert_eq!(base_j, 0);

        let run_id = Spi::get_one::<i32>(&format!("SELECT epanet_simulate_scenario({sid})"))
            .unwrap()
            .unwrap();
        assert!(run_id > 0);

        let _ = Spi::get_one::<bool>(&format!("SELECT epanet_delete({nid})")).unwrap();
    }

    #[pg_test]
    fn test_epanet_add_junction_base() {
        let inp = r#"[JUNCTIONS]
J1 100.0 10.0
[RESERVOIRS]
R1 150.0
[PIPES]
P1 J1 R1 100.0 200.0 100.0
[COORDINATES]
J1 0.0 0.0
R1 0.0 100.0
"#;
        let nid = Spi::get_one::<i32>(&format!(
            "SELECT epanet_import('topo_base', $${inp}$$, 4326)"
        ))
        .unwrap()
        .unwrap();

        Spi::run(&format!(
            "SELECT epanet_add_junction({nid}, 'J2', 95.0, 5.0, 100.0, 0.0, NULL)"
        ))
        .unwrap();

        let n = Spi::get_one::<i64>(&format!(
            "SELECT count(*)::bigint FROM epanet.junctions WHERE network_id = {nid} AND name = 'J2'"
        ))
        .unwrap()
        .unwrap();
        assert_eq!(n, 1);

        let geom = Spi::get_one::<bool>(&format!(
            "SELECT geom IS NOT NULL FROM epanet.junctions WHERE network_id = {nid} AND name = 'J2'"
        ))
        .unwrap()
        .unwrap();
        assert!(geom);

        let _ = Spi::get_one::<bool>(&format!("SELECT epanet_delete({nid})")).unwrap();
    }

    #[pg_test]
    fn test_epanet_create_network_from_scratch() {
        let nid = Spi::get_one::<i32>(&format!(
            "SELECT epanet_create_network('scratch_net', 4326)"
        ))
        .unwrap()
        .unwrap();

        Spi::run(&format!(
            "SELECT epanet_add_reservoir({nid}, 'R1', 150.0, 0.0, 100.0, NULL)"
        ))
        .unwrap();
        Spi::run(&format!(
            "SELECT epanet_add_junction({nid}, 'J1', 100.0, 10.0, 100.0, 0.0, NULL)"
        ))
        .unwrap();
        Spi::run(&format!(
            "SELECT epanet_add_pipe({nid}, 'P1', 'R1', 'J1', 100.0, 200.0, 100.0, 0.0, 'Open')"
        ))
        .unwrap();
        Spi::run(&format!(
            "SELECT epanet_set_times({nid}, 'Duration', '1:00')"
        ))
        .unwrap();
        Spi::run(&format!(
            "SELECT epanet_set_times({nid}, 'Hydraulic Timestep', '1:00')"
        ))
        .unwrap();
        Spi::run(&format!(
            "SELECT epanet_set_times({nid}, 'Report Timestep', '1:00')"
        ))
        .unwrap();
        Spi::run(&format!("SELECT epanet_refresh_inp({nid})")).unwrap();

        let run_id = Spi::get_one::<i32>(&format!("SELECT epanet_simulate({nid})"))
            .unwrap()
            .unwrap();
        assert!(run_id > 0);

        let n_nodes = Spi::get_one::<i64>(&format!(
            "SELECT count(*)::bigint FROM epanet.node_results WHERE run_id = {run_id}"
        ))
        .unwrap()
        .unwrap();
        assert!(n_nodes > 0);

        let _ = Spi::get_one::<bool>(&format!("SELECT epanet_delete({nid})")).unwrap();
    }

    #[pg_test]
    fn test_epanet_set_node_coordinates_cascades_links() {
        let inp = r#"[JUNCTIONS]
J1 100.0 10.0
J2 90.0 0.0
[RESERVOIRS]
R1 150.0
[PIPES]
P1 J1 J2 100.0 200.0 100.0
[COORDINATES]
J1 0.0 0.0
J2 100.0 0.0
R1 0.0 100.0
"#;
        let nid = Spi::get_one::<i32>(&format!(
            "SELECT epanet_import('move_test', $${inp}$$, 4326)"
        ))
        .unwrap()
        .unwrap();

        Spi::run(&format!(
            "SELECT epanet_set_node_coordinates({nid}, 'J2', 200.0, 0.0)"
        ))
        .unwrap();

        let x = Spi::get_one::<f64>(&format!(
            "SELECT x FROM epanet.coordinates WHERE network_id = {nid} AND node_id = 'J2'"
        ))
        .unwrap()
        .unwrap();
        assert!((x - 200.0).abs() < 1e-6);

        let pipe_ok = Spi::get_one::<bool>(&format!(
            "SELECT ST_NPoints(geom) >= 2 FROM epanet.pipes WHERE network_id = {nid} AND name = 'P1'"
        ))
        .unwrap()
        .unwrap();
        assert!(pipe_ok);

        let _ = Spi::get_one::<bool>(&format!("SELECT epanet_delete({nid})")).unwrap();
    }

    #[pg_test]
    fn test_epanet_links_view() {
        let nid = Spi::get_one::<i32>(&format!(
            "SELECT epanet_create_network('links_view', 4326)"
        ))
        .unwrap()
        .unwrap();
        Spi::run(&format!(
            "SELECT epanet_add_reservoir({nid}, 'R1', 150.0, 0.0, 0.0)"
        ))
        .unwrap();
        Spi::run(&format!(
            "SELECT epanet_add_junction({nid}, 'J1', 100.0, 0.0, 100.0, 0.0)"
        ))
        .unwrap();
        Spi::run(&format!(
            "SELECT epanet_add_pipe({nid}, 'P1', 'R1', 'J1', 100.0, 200.0, 100.0)"
        ))
        .unwrap();

        let n = Spi::get_one::<i64>(&format!(
            "SELECT count(*)::bigint FROM epanet.links WHERE network_id = {nid}"
        ))
        .unwrap()
        .unwrap();
        assert_eq!(n, 1);

        let _ = Spi::get_one::<bool>(&format!("SELECT epanet_delete({nid})")).unwrap();
    }

    #[pg_test]
    fn test_epanet_scenario_map_geom() {
        let inp = r#"[JUNCTIONS]
J1 100.0 10.0
[RESERVOIRS]
R1 150.0
[PIPES]
P1 J1 R1 100.0 200.0 100.0
[COORDINATES]
J1 0.0 0.0
R1 0.0 100.0
"#;
        let nid = Spi::get_one::<i32>(&format!(
            "SELECT epanet_import('scn_map', $${inp}$$, 4326)"
        ))
        .unwrap()
        .unwrap();
        let sid = Spi::get_one::<i32>(&format!(
            "SELECT epanet_create_scenario({nid}, 'ext', NULL, 1.0)"
        ))
        .unwrap()
        .unwrap();

        Spi::run(&format!(
            "SELECT epanet_add_scenario_junction({sid}, 'J2', 95.0, 5.0, 100.0, 0.0, NULL)"
        ))
        .unwrap();
        Spi::run(&format!(
            "SELECT epanet_add_scenario_pipe({sid}, 'P2', 'J2', 'R1', 50.0, 150.0, 100.0, 0.0, 'Open')"
        ))
        .unwrap();

        let has_geom = Spi::get_one::<bool>(&format!(
            "SELECT geom IS NOT NULL FROM epanet_scenario_links({sid}) \
             WHERE link_id = 'P2' AND provisional"
        ))
        .unwrap()
        .unwrap();
        assert!(has_geom);

        let node_count = Spi::get_one::<i64>(&format!(
            "SELECT count(*)::bigint FROM epanet_scenario_nodes({sid})"
        ))
        .unwrap()
        .unwrap();
        assert!(node_count >= 3);

        let _ = Spi::get_one::<bool>(&format!("SELECT epanet_delete({nid})")).unwrap();
    }
}

#[cfg(feature = "pg_bench")]
#[pg_schema]
mod benches {
    use pgrx::prelude::*;
    use pgrx_bench::{Bencher, black_box};

    #[pg_bench]
    fn bench_hello_pg_epanet(b: &mut Bencher) {
        b.iter(|| {
            black_box(crate::hello_pg_epanet());
        });
    }
}

/// This module is required by `cargo pgrx test` invocations.
/// It must be visible at the root of your extension crate.
#[cfg(test)]
pub mod pg_test {
    pub fn setup(_options: Vec<&str>) {}

    #[must_use]
    pub fn postgresql_conf_options() -> Vec<&'static str> {
        vec![]
    }
}

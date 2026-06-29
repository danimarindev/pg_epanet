//! Network topology editing — base tables and scenario-only provisional elements.

use pgrx::prelude::*;
use pgrx::spi::SpiResult;

use crate::export;
use crate::scenario;
use crate::sql_text;

fn network_srid(network_id: i32) -> i32 {
    Spi::get_one::<i32>(&format!("SELECT srid FROM epanet.networks WHERE id = {network_id}"))
        .unwrap()
        .unwrap_or_else(|| error!("No network found with id={network_id}"))
}

fn assert_network(network_id: i32) {
    let exists = Spi::get_one::<bool>(&format!(
        "SELECT EXISTS(SELECT 1 FROM epanet.networks WHERE id = {network_id})"
    ))
    .unwrap()
    .unwrap_or(false);
    if !exists {
        error!("No network found with id={network_id}");
    }
}

fn assert_scenario(scenario_id: i32) -> i32 {
    Spi::get_one::<i32>(&format!(
        "SELECT network_id FROM epanet.scenarios WHERE id = {scenario_id}"
    ))
    .unwrap()
    .unwrap_or_else(|| error!("No scenario found with id={scenario_id}"))
}

fn node_exists(network_id: i32, name: &str) -> bool {
    Spi::get_one::<bool>(&format!(
        "SELECT EXISTS (
            SELECT 1 FROM epanet.junctions WHERE network_id = {network_id} AND name = {}
            UNION ALL
            SELECT 1 FROM epanet.tanks WHERE network_id = {network_id} AND name = {}
            UNION ALL
            SELECT 1 FROM epanet.reservoirs WHERE network_id = {network_id} AND name = {}
        )",
        sql_text(name),
        sql_text(name),
        sql_text(name)
    ))
    .unwrap()
    .unwrap_or(false)
}

fn node_exists_for_scenario(scenario_id: i32, network_id: i32, name: &str) -> bool {
    if node_exists(network_id, name) {
        return true;
    }
    Spi::get_one::<bool>(&format!(
        "SELECT EXISTS (
            SELECT 1 FROM epanet.scenario_elements \
            WHERE scenario_id = {scenario_id} \
              AND element_type IN ('junction', 'reservoir', 'tank') \
              AND name = {}
        )",
        sql_text(name)
    ))
    .unwrap()
    .unwrap_or(false)
}

pub(crate) fn update_point_geom(table: &str, network_id: i32, name: &str, srid: i32) {
    Spi::run(&format!(
        "UPDATE epanet.{table} t \
         SET geom = ST_SetSRID(ST_MakePoint(c.x, c.y), {srid}) \
         FROM epanet.coordinates c \
         WHERE t.network_id = c.network_id AND t.name = c.node_id \
           AND t.network_id = {network_id} AND t.name = {}",
        sql_text(name)
    ))
    .unwrap_or_else(|e| error!("SPI error updating {table} geometry: {e:?}"));
}

fn update_junction_geom(network_id: i32, name: &str, srid: i32) {
    update_point_geom("junctions", network_id, name, srid);
}

fn update_reservoir_geom(network_id: i32, name: &str, srid: i32) {
    update_point_geom("reservoirs", network_id, name, srid);
}

fn update_tank_geom(network_id: i32, name: &str, srid: i32) {
    update_point_geom("tanks", network_id, name, srid);
}

pub(crate) fn update_direct_link_geom(table: &str, network_id: i32, name: &str, srid: i32) {
    Spi::run(&format!(
        "UPDATE epanet.{table} l \
         SET geom = ST_SetSRID(ST_MakeLine(ST_MakePoint(c1.x, c1.y), ST_MakePoint(c2.x, c2.y)), {srid}) \
         FROM epanet.coordinates c1, epanet.coordinates c2 \
         WHERE l.network_id = c1.network_id AND l.node1 = c1.node_id \
           AND l.network_id = c2.network_id AND l.node2 = c2.node_id \
           AND l.network_id = {network_id} AND l.name = {}",
        sql_text(name)
    ))
    .unwrap_or_else(|e| error!("SPI error updating {table} geometry: {e:?}"));
}

pub(crate) fn update_pipe_geom(network_id: i32, name: &str, srid: i32) {
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
           AND p.network_id = {network_id} AND p.name = {}",
        sql_text(name)
    ))
    .unwrap_or_else(|e| error!("SPI error updating pipe geometry: {e:?}"));
}

/// Adds a junction to the base network (tables only — use `epanet_refresh_inp` to sync INP text).
pub fn add_junction(
    network_id: i32,
    name: &str,
    elevation: f64,
    demand: f64,
    x: f64,
    y: f64,
    pattern: Option<&str>,
) {
    assert_network(network_id);
    let srid = network_srid(network_id);
    let pat_sql = match pattern {
        Some(p) => sql_text(p),
        None => "NULL".into(),
    };
    Spi::run(&format!(
        "INSERT INTO epanet.junctions(network_id, name, elevation, demand, pattern) \
         VALUES ({network_id}, {}, {elevation}, {demand}, {pat_sql})",
        sql_text(name)
    ))
    .unwrap_or_else(|e| error!("SPI error inserting junction: {e:?}"));
    Spi::run(&format!(
        "INSERT INTO epanet.coordinates(network_id, node_id, x, y) \
         VALUES ({network_id}, {}, {x}, {y})",
        sql_text(name)
    ))
    .unwrap_or_else(|e| error!("SPI error inserting coordinate: {e:?}"));
    update_junction_geom(network_id, name, srid);
}

/// Adds a pipe to the base network.
pub fn add_pipe(
    network_id: i32,
    name: &str,
    node1: &str,
    node2: &str,
    length: f64,
    diameter: f64,
    roughness: f64,
    minor_loss: f64,
    status: &str,
) {
    assert_network(network_id);
    if !node_exists(network_id, node1) {
        error!("node1 '{node1}' not found in network {network_id}");
    }
    if !node_exists(network_id, node2) {
        error!("node2 '{node2}' not found in network {network_id}");
    }
    let srid = network_srid(network_id);
    Spi::run(&format!(
        "INSERT INTO epanet.pipes(network_id, name, node1, node2, length, diameter, roughness, minor_loss, status) \
         VALUES ({network_id}, {}, {}, {}, {length}, {diameter}, {roughness}, {minor_loss}, {})",
        sql_text(name),
        sql_text(node1),
        sql_text(node2),
        sql_text(status)
    ))
    .unwrap_or_else(|e| error!("SPI error inserting pipe: {e:?}"));
    update_pipe_geom(network_id, name, srid);
}

/// Adds a reservoir to the base network.
pub fn add_reservoir(
    network_id: i32,
    name: &str,
    head: f64,
    x: f64,
    y: f64,
    pattern: Option<&str>,
) {
    assert_network(network_id);
    let srid = network_srid(network_id);
    let pat_sql = match pattern {
        Some(p) => sql_text(p),
        None => "NULL".into(),
    };
    Spi::run(&format!(
        "INSERT INTO epanet.reservoirs(network_id, name, head, pattern) \
         VALUES ({network_id}, {}, {head}, {pat_sql})",
        sql_text(name)
    ))
    .unwrap_or_else(|e| error!("SPI error inserting reservoir: {e:?}"));
    Spi::run(&format!(
        "INSERT INTO epanet.coordinates(network_id, node_id, x, y) \
         VALUES ({network_id}, {}, {x}, {y}) \
         ON CONFLICT (network_id, node_id) DO UPDATE SET x = EXCLUDED.x, y = EXCLUDED.y",
        sql_text(name)
    ))
    .unwrap_or_else(|e| error!("SPI error inserting coordinate: {e:?}"));
    update_reservoir_geom(network_id, name, srid);
}

/// Adds a tank to the base network.
#[allow(clippy::too_many_arguments)]
pub fn add_tank(
    network_id: i32,
    name: &str,
    elevation: f64,
    init_level: f64,
    min_level: f64,
    max_level: f64,
    diameter: f64,
    min_volume: f64,
    x: f64,
    y: f64,
    volume_curve: Option<&str>,
    overflow: Option<&str>,
) {
    assert_network(network_id);
    let srid = network_srid(network_id);
    let vc_sql = match volume_curve {
        Some(v) => sql_text(v),
        None => "NULL".into(),
    };
    let ov_sql = match overflow {
        Some(o) => sql_text(o),
        None => "NULL".into(),
    };
    Spi::run(&format!(
        "INSERT INTO epanet.tanks(network_id, name, elevation, init_level, min_level, max_level, \
         diameter, min_volume, volume_curve, overflow) \
         VALUES ({network_id}, {}, {elevation}, {init_level}, {min_level}, {max_level}, \
         {diameter}, {min_volume}, {vc_sql}, {ov_sql})",
        sql_text(name)
    ))
    .unwrap_or_else(|e| error!("SPI error inserting tank: {e:?}"));
    Spi::run(&format!(
        "INSERT INTO epanet.coordinates(network_id, node_id, x, y) \
         VALUES ({network_id}, {}, {x}, {y}) \
         ON CONFLICT (network_id, node_id) DO UPDATE SET x = EXCLUDED.x, y = EXCLUDED.y",
        sql_text(name)
    ))
    .unwrap_or_else(|e| error!("SPI error inserting coordinate: {e:?}"));
    update_tank_geom(network_id, name, srid);
}

/// Adds a pump to the base network. `pump_type` is `HEAD` or `POWER`.
#[allow(clippy::too_many_arguments)]
pub fn add_pump(
    network_id: i32,
    name: &str,
    node1: &str,
    node2: &str,
    pump_type: &str,
    head_curve: Option<&str>,
    power: Option<f64>,
    speed: Option<f64>,
    pattern: Option<&str>,
) {
    assert_network(network_id);
    if !node_exists(network_id, node1) {
        error!("node1 '{node1}' not found in network {network_id}");
    }
    if !node_exists(network_id, node2) {
        error!("node2 '{node2}' not found in network {network_id}");
    }
    let srid = network_srid(network_id);
    let pt = pump_type.to_ascii_uppercase();
    let hc_sql = head_curve.map(sql_text).unwrap_or_else(|| "NULL".into());
    let pw_sql = power.map(|p| format!("{p}")).unwrap_or_else(|| "NULL".into());
    let sp_sql = speed.map(|s| format!("{s}")).unwrap_or_else(|| "NULL".into());
    let pat_sql = pattern.map(sql_text).unwrap_or_else(|| "NULL".into());
    Spi::run(&format!(
        "INSERT INTO epanet.pumps(network_id, name, node1, node2, pump_type, head_curve, power, speed, pattern) \
         VALUES ({network_id}, {}, {}, {}, {}, {hc_sql}, {pw_sql}, {sp_sql}, {pat_sql})",
        sql_text(name),
        sql_text(node1),
        sql_text(node2),
        sql_text(&pt)
    ))
    .unwrap_or_else(|e| error!("SPI error inserting pump: {e:?}"));
    update_direct_link_geom("pumps", network_id, name, srid);
}

/// Adds a valve to the base network.
pub fn add_valve(
    network_id: i32,
    name: &str,
    node1: &str,
    node2: &str,
    diameter: f64,
    valve_type: &str,
    setting: &str,
    minor_loss: f64,
) {
    assert_network(network_id);
    if !node_exists(network_id, node1) {
        error!("node1 '{node1}' not found in network {network_id}");
    }
    if !node_exists(network_id, node2) {
        error!("node2 '{node2}' not found in network {network_id}");
    }
    let srid = network_srid(network_id);
    Spi::run(&format!(
        "INSERT INTO epanet.valves(network_id, name, node1, node2, diameter, valve_type, setting, minor_loss) \
         VALUES ({network_id}, {}, {}, {}, {diameter}, {}, {}, {minor_loss})",
        sql_text(name),
        sql_text(node1),
        sql_text(node2),
        sql_text(valve_type),
        sql_text(setting)
    ))
    .unwrap_or_else(|e| error!("SPI error inserting valve: {e:?}"));
    update_direct_link_geom("valves", network_id, name, srid);
}

/// Adds a provisional junction visible only in scenario simulations.
pub fn add_scenario_junction(
    scenario_id: i32,
    name: &str,
    elevation: f64,
    demand: f64,
    x: f64,
    y: f64,
    pattern: Option<&str>,
) {
    let network_id = assert_scenario(scenario_id);
    let _ = network_id;
    let fields = match pattern {
        Some(p) => format!("{elevation} {demand} {p}"),
        None => format!("{elevation} {demand}"),
    };
    let srid = network_srid(network_id);
    Spi::run(&format!(
        "INSERT INTO epanet.scenario_elements(scenario_id, element_type, name, inp_fields, coord_x, coord_y, geom) \
         VALUES ({scenario_id}, 'junction', {}, {}, {x}, {y}, ST_SetSRID(ST_MakePoint({x}, {y}), {srid}))",
        sql_text(name),
        sql_text(&fields)
    ))
    .unwrap_or_else(|e| error!("SPI error inserting scenario junction: {e:?}"));
}

/// Adds a provisional pipe visible only in scenario simulations.
pub fn add_scenario_pipe(
    scenario_id: i32,
    name: &str,
    node1: &str,
    node2: &str,
    length: f64,
    diameter: f64,
    roughness: f64,
    minor_loss: f64,
    status: &str,
) {
    let network_id = assert_scenario(scenario_id);
    if !node_exists_for_scenario(scenario_id, network_id, node1) {
        error!("node1 '{node1}' not found in base network or scenario elements");
    }
    if !node_exists_for_scenario(scenario_id, network_id, node2) {
        error!("node2 '{node2}' not found in base network or scenario elements");
    }
    let fields = format!(
        "{node1} {node2} {length} {diameter} {roughness} {minor_loss} {status}"
    );
    Spi::run(&format!(
        "INSERT INTO epanet.scenario_elements(scenario_id, element_type, name, inp_fields) \
         VALUES ({scenario_id}, 'pipe', {}, {})",
        sql_text(name),
        sql_text(&fields)
    ))
    .unwrap_or_else(|e| error!("SPI error inserting scenario pipe: {e:?}"));
    refresh_scenario_link_geom(scenario_id, name);
}

/// Adds a provisional reservoir visible only in scenario simulations.
pub fn add_scenario_reservoir(
    scenario_id: i32,
    name: &str,
    head: f64,
    x: f64,
    y: f64,
    pattern: Option<&str>,
) {
    let network_id = assert_scenario(scenario_id);
    let srid = network_srid(network_id);
    let fields = match pattern {
        Some(p) => format!("{head} {p}"),
        None => format!("{head}"),
    };
    Spi::run(&format!(
        "INSERT INTO epanet.scenario_elements(scenario_id, element_type, name, inp_fields, coord_x, coord_y, geom) \
         VALUES ({scenario_id}, 'reservoir', {}, {}, {x}, {y}, ST_SetSRID(ST_MakePoint({x}, {y}), {srid}))",
        sql_text(name),
        sql_text(&fields)
    ))
    .unwrap_or_else(|e| error!("SPI error inserting scenario reservoir: {e:?}"));
}

/// Adds a provisional tank visible only in scenario simulations.
#[allow(clippy::too_many_arguments)]
pub fn add_scenario_tank(
    scenario_id: i32,
    name: &str,
    elevation: f64,
    init_level: f64,
    min_level: f64,
    max_level: f64,
    diameter: f64,
    min_volume: f64,
    x: f64,
    y: f64,
    volume_curve: Option<&str>,
    overflow: Option<&str>,
) {
    let network_id = assert_scenario(scenario_id);
    let srid = network_srid(network_id);
    let mut fields = format!(
        "{elevation} {init_level} {min_level} {max_level} {diameter} {min_volume}"
    );
    if let Some(c) = volume_curve {
        fields.push(' ');
        fields.push_str(c);
    }
    if let Some(o) = overflow {
        fields.push(' ');
        fields.push_str(o);
    }
    Spi::run(&format!(
        "INSERT INTO epanet.scenario_elements(scenario_id, element_type, name, inp_fields, coord_x, coord_y, geom) \
         VALUES ({scenario_id}, 'tank', {}, {}, {x}, {y}, ST_SetSRID(ST_MakePoint({x}, {y}), {srid}))",
        sql_text(name),
        sql_text(&fields)
    ))
    .unwrap_or_else(|e| error!("SPI error inserting scenario tank: {e:?}"));
}

/// Adds a provisional pump visible only in scenario simulations.
pub fn add_scenario_pump(
    scenario_id: i32,
    name: &str,
    node1: &str,
    node2: &str,
    pump_type: &str,
    head_curve: Option<&str>,
    power: Option<f64>,
    speed: Option<f64>,
    pattern: Option<&str>,
) {
    let network_id = assert_scenario(scenario_id);
    if !node_exists_for_scenario(scenario_id, network_id, node1) {
        error!("node1 '{node1}' not found in base network or scenario elements");
    }
    if !node_exists_for_scenario(scenario_id, network_id, node2) {
        error!("node2 '{node2}' not found in base network or scenario elements");
    }
    let mut fields = format!("{node1} {node2}");
    match pump_type.to_ascii_uppercase().as_str() {
        "HEAD" => {
            fields.push_str(" HEAD");
            if let Some(h) = head_curve {
                fields.push(' ');
                fields.push_str(h);
            }
        }
        "POWER" => {
            fields.push_str(" POWER");
            if let Some(p) = power {
                fields.push(' ');
                fields.push_str(&format!("{p}"));
            }
            if let Some(s) = speed {
                fields.push_str(&format!(" SPEED {s}"));
            }
            if let Some(pat) = pattern {
                fields.push_str(&format!(" PATTERN {pat}"));
            }
        }
        other => error!("pump_type must be HEAD or POWER, got '{other}'"),
    }
    Spi::run(&format!(
        "INSERT INTO epanet.scenario_elements(scenario_id, element_type, name, inp_fields) \
         VALUES ({scenario_id}, 'pump', {}, {})",
        sql_text(name),
        sql_text(&fields)
    ))
    .unwrap_or_else(|e| error!("SPI error inserting scenario pump: {e:?}"));
    refresh_scenario_link_geom(scenario_id, name);
}

/// Adds a provisional valve visible only in scenario simulations.
pub fn add_scenario_valve(
    scenario_id: i32,
    name: &str,
    node1: &str,
    node2: &str,
    diameter: f64,
    valve_type: &str,
    setting: &str,
    minor_loss: f64,
) {
    let network_id = assert_scenario(scenario_id);
    if !node_exists_for_scenario(scenario_id, network_id, node1) {
        error!("node1 '{node1}' not found in base network or scenario elements");
    }
    if !node_exists_for_scenario(scenario_id, network_id, node2) {
        error!("node2 '{node2}' not found in base network or scenario elements");
    }
    let fields = format!(
        "{node1} {node2} {diameter} {valve_type} {setting} {minor_loss}"
    );
    Spi::run(&format!(
        "INSERT INTO epanet.scenario_elements(scenario_id, element_type, name, inp_fields) \
         VALUES ({scenario_id}, 'valve', {}, {})",
        sql_text(name),
        sql_text(&fields)
    ))
    .unwrap_or_else(|e| error!("SPI error inserting scenario valve: {e:?}"));
    refresh_scenario_link_geom(scenario_id, name);
}

pub fn refresh_scenario_link_geom(scenario_id: i32, link_name: &str) {
    let network_id = assert_scenario(scenario_id);
    let srid = network_srid(network_id);
    let lit = sql_text(link_name);

    Spi::run(&format!(
        "UPDATE epanet.scenario_elements se \
         SET geom = ST_SetSRID( \
             ST_MakeLine( \
                 ARRAY(SELECT ST_MakePoint(n1.x, n1.y) \
                       FROM epanet.effective_node_xy({scenario_id}, split_part(se.inp_fields, ' ', 1)) n1) \
                 || ARRAY(SELECT ST_MakePoint(v.x, v.y) \
                          FROM epanet.scenario_element_vertices v \
                          WHERE v.scenario_id = {scenario_id} AND v.link_id = se.name \
                          ORDER BY v.idx) \
                 || ARRAY(SELECT ST_MakePoint(n2.x, n2.y) \
                       FROM epanet.effective_node_xy({scenario_id}, split_part(se.inp_fields, ' ', 2)) n2) \
             ), {srid}) \
         WHERE se.scenario_id = {scenario_id} AND se.name = {lit} \
           AND se.element_type IN ('pipe', 'pump', 'valve')"
    ))
    .unwrap_or_else(|e| error!("SPI error refreshing scenario link geom: {e:?}"));
    let _ = network_id;
}

fn refresh_scenario_links_at_node(scenario_id: i32, node_id: &str) {
    let lit = sql_text(node_id);
    let mut links: Vec<String> = Vec::new();
    let _ = Spi::connect(|client| -> SpiResult<_> {
        let rows = client.select(
            &format!(
                "SELECT name FROM epanet.scenario_elements \
                 WHERE scenario_id = {scenario_id} \
                   AND element_type IN ('pipe', 'pump', 'valve') \
                   AND (split_part(inp_fields, ' ', 1) = {lit} \
                        OR split_part(inp_fields, ' ', 2) = {lit})"
            ),
            None,
            None,
        )?;
        for row in rows {
            links.push(row.get_by_name("name")?.unwrap());
        }
        Ok(())
    });
    for name in links {
        refresh_scenario_link_geom(scenario_id, &name);
    }
}

pub fn set_scenario_node_coordinates(scenario_id: i32, node_id: &str, x: f64, y: f64) {
    let network_id = assert_scenario(scenario_id);
    let srid = network_srid(network_id);

    let updated = Spi::get_one::<bool>(&format!(
        "UPDATE epanet.scenario_elements \
         SET coord_x = {x}, coord_y = {y}, \
             geom = ST_SetSRID(ST_MakePoint({x}, {y}), {srid}) \
         WHERE scenario_id = {scenario_id} AND name = {} \
           AND element_type IN ('junction', 'reservoir', 'tank') \
         RETURNING true",
        sql_text(node_id)
    ))
    .unwrap_or_else(|e| error!("SPI error updating scenario node: {e:?}"))
    .unwrap_or(false);

    if !updated {
        error!("scenario node '{node_id}' not found in scenario {scenario_id}");
    }

    refresh_scenario_links_at_node(scenario_id, node_id);
    let _ = network_id;
}

pub fn add_scenario_vertex(scenario_id: i32, link_id: &str, x: f64, y: f64) {
    let _ = assert_scenario(scenario_id);
    Spi::run(&format!(
        "INSERT INTO epanet.scenario_element_vertices(scenario_id, link_id, idx, x, y) \
         VALUES ({scenario_id}, {}, \
         (SELECT COALESCE(MAX(idx), -1) + 1 FROM epanet.scenario_element_vertices \
          WHERE scenario_id = {scenario_id} AND link_id = {}), {x}, {y})",
        sql_text(link_id),
        sql_text(link_id)
    ))
    .unwrap_or_else(|e| error!("SPI error inserting scenario vertex: {e:?}"));
    refresh_scenario_link_geom(scenario_id, link_id);
}

pub fn refresh_scenario_geoms(scenario_id: i32) {
    let network_id = assert_scenario(scenario_id);
    let srid = network_srid(network_id);

    Spi::run(&format!(
        "UPDATE epanet.scenario_elements \
         SET geom = ST_SetSRID(ST_MakePoint(coord_x, coord_y), {srid}) \
         WHERE scenario_id = {scenario_id} \
           AND element_type IN ('junction', 'reservoir', 'tank') \
           AND coord_x IS NOT NULL AND coord_y IS NOT NULL"
    ))
    .ok();

    let mut links: Vec<String> = Vec::new();
    let _ = Spi::connect(|client| -> SpiResult<_> {
        let rows = client.select(
            &format!(
                "SELECT name FROM epanet.scenario_elements \
                 WHERE scenario_id = {scenario_id} \
                   AND element_type IN ('pipe', 'pump', 'valve')"
            ),
            None,
            None,
        )?;
        for row in rows {
            links.push(row.get_by_name("name")?.unwrap());
        }
        Ok(())
    });
    for name in links {
        refresh_scenario_link_geom(scenario_id, &name);
    }
    let _ = network_id;
}

pub fn remove_element(network_id: i32, element_type: &str, name: &str) -> bool {
    assert_network(network_id);
    let table = match element_type.to_ascii_lowercase().as_str() {
        "junction" => "junctions",
        "pipe" => "pipes",
        "pump" => "pumps",
        "valve" => "valves",
        "tank" => "tanks",
        "reservoir" => "reservoirs",
        _ => error!("Unknown element_type '{element_type}'"),
    };
    Spi::get_one::<bool>(&format!(
        "DELETE FROM epanet.{table} WHERE network_id = {network_id} AND name = {} RETURNING true",
        sql_text(name)
    ))
    .unwrap_or_else(|e| error!("SPI error removing element: {e:?}"))
    .unwrap_or_else(|| error!("No {element_type} '{name}' in network {network_id}"))
}

pub fn remove_scenario_element(scenario_id: i32, element_type: &str, name: &str) -> bool {
    let _ = assert_scenario(scenario_id);
    Spi::run(&format!(
        "DELETE FROM epanet.scenario_element_vertices \
         WHERE scenario_id = {scenario_id} AND link_id = {}",
        sql_text(name)
    ))
    .ok();
    Spi::get_one::<bool>(&format!(
        "DELETE FROM epanet.scenario_elements \
         WHERE scenario_id = {scenario_id} AND element_type = {} AND name = {} RETURNING true",
        sql_text(element_type),
        sql_text(name)
    ))
    .unwrap_or_else(|e| error!("SPI error removing scenario element: {e:?}"))
    .unwrap_or_else(|| error!("No scenario {element_type} '{name}'"))
}

pub fn connect_nodes(
    network_id: i32,
    link_type: &str,
    link_name: &str,
    node1: &str,
    node2: &str,
) -> bool {
    assert_network(network_id);
    if !node_exists(network_id, node1) || !node_exists(network_id, node2) {
        error!("Both endpoint nodes must exist in network {network_id}");
    }
    let table = match link_type.to_ascii_lowercase().as_str() {
        "pipe" => "pipes",
        "pump" => "pumps",
        "valve" => "valves",
        _ => error!("link_type must be pipe, pump, or valve"),
    };
    Spi::get_one::<bool>(&format!(
        "UPDATE epanet.{table} SET node1 = {}, node2 = {} \
         WHERE network_id = {network_id} AND name = {} RETURNING true",
        sql_text(node1),
        sql_text(node2),
        sql_text(link_name)
    ))
    .unwrap_or_else(|e| error!("SPI error reconnecting link: {e:?}"))
    .unwrap_or_else(|| error!("No {link_type} '{link_name}' in network {network_id}"))
}

/// Promotes scenario elements and overrides into the base network, then refreshes INP.
pub fn merge_scenario_into_base(scenario_id: i32) -> i32 {
    let network_id = assert_scenario(scenario_id);
    let srid = network_srid(network_id);

    let mut elements: Vec<(String, String, String, Option<f64>, Option<f64>)> = Vec::new();
    let _ = Spi::connect(|client| -> SpiResult<_> {
        let rows = client.select(
            &format!(
                "SELECT element_type, name, inp_fields, coord_x, coord_y \
                 FROM epanet.scenario_elements WHERE scenario_id = {scenario_id}"
            ),
            None,
            None,
        )?;
        for row in rows {
            elements.push((
                row.get_by_name("element_type")?.unwrap(),
                row.get_by_name("name")?.unwrap(),
                row.get_by_name("inp_fields")?.unwrap(),
                row.get_by_name("coord_x")?,
                row.get_by_name("coord_y")?,
            ));
        }
        Ok(())
    });

    for (etype, name, fields, cx, cy) in &elements {
        match etype.as_str() {
            "junction" => {
                let parts: Vec<&str> = fields.split_whitespace().collect();
                let elevation: f64 = parts.first().and_then(|s| s.parse().ok()).unwrap_or(0.0);
                let demand: f64 = parts.get(1).and_then(|s| s.parse().ok()).unwrap_or(0.0);
                let pattern = parts.get(2).map(|s| *s);
                let x = cx.ok_or_else(|| error!("scenario junction '{name}' missing coord_x"))?;
                let y = cy.ok_or_else(|| error!("scenario junction '{name}' missing coord_y"))?;
                add_junction(network_id, name, elevation, demand, x, y, pattern);
            }
            "pipe" => {
                let parts: Vec<&str> = fields.split_whitespace().collect();
                if parts.len() < 7 {
                    error!("Invalid scenario pipe fields for '{name}'");
                }
                add_pipe(
                    network_id,
                    name,
                    parts[0],
                    parts[1],
                    parts[2].parse().unwrap_or(0.0),
                    parts[3].parse().unwrap_or(0.0),
                    parts[4].parse().unwrap_or(100.0),
                    parts[5].parse().unwrap_or(0.0),
                    parts[6],
                );
            }
            "reservoir" => {
                let parts: Vec<&str> = fields.split_whitespace().collect();
                let head: f64 = parts.first().and_then(|s| s.parse().ok()).unwrap_or(0.0);
                let pattern = parts.get(1).map(|s| *s);
                let x = cx.ok_or_else(|| error!("scenario reservoir '{name}' missing coord_x"))?;
                let y = cy.ok_or_else(|| error!("scenario reservoir '{name}' missing coord_y"))?;
                add_reservoir(network_id, name, head, x, y, pattern);
            }
            "tank" => {
                let parts: Vec<&str> = fields.split_whitespace().collect();
                if parts.len() < 6 {
                    error!("Invalid scenario tank fields for '{name}'");
                }
                let x = cx.ok_or_else(|| error!("scenario tank '{name}' missing coord_x"))?;
                let y = cy.ok_or_else(|| error!("scenario tank '{name}' missing coord_y"))?;
                add_tank(
                    network_id,
                    name,
                    parts[0].parse().unwrap_or(0.0),
                    parts[1].parse().unwrap_or(0.0),
                    parts[2].parse().unwrap_or(0.0),
                    parts[3].parse().unwrap_or(0.0),
                    parts[4].parse().unwrap_or(0.0),
                    parts[5].parse().unwrap_or(0.0),
                    x,
                    y,
                    parts.get(6).map(|s| *s),
                    parts.get(7).map(|s| *s),
                );
            }
            "pump" => merge_scenario_pump(network_id, name, fields),
            "valve" => {
                let parts: Vec<&str> = fields.split_whitespace().collect();
                if parts.len() < 6 {
                    error!("Invalid scenario valve fields for '{name}'");
                }
                add_valve(
                    network_id,
                    name,
                    parts[0],
                    parts[1],
                    parts[2].parse().unwrap_or(0.0),
                    parts[3],
                    parts[4],
                    parts[5].parse().unwrap_or(0.0),
                );
            }
            _ => warning!("epanet: skipping merge of scenario element type '{etype}'"),
        }
    }

    let (_, _, overrides) = scenario::load_scenario(scenario_id);
    for ov in &overrides {
        merge_override_to_base(network_id, ov);
    }

    export::refresh_inp_text(network_id);
    network_id
}

fn merge_scenario_pump(network_id: i32, name: &str, fields: &str) {
    let parts: Vec<&str> = fields.split_whitespace().collect();
    if parts.len() < 4 {
        error!("Invalid scenario pump fields for '{name}'");
    }
    let node1 = parts[0];
    let node2 = parts[1];
    match parts[2].to_ascii_uppercase().as_str() {
        "HEAD" => {
            let curve = parts.get(3).map(|s| *s);
            add_pump(network_id, name, node1, node2, "HEAD", curve, None, None, None);
        }
        "POWER" => {
            let power = parts.get(3).and_then(|s| s.parse().ok());
            let mut speed = None;
            let mut pattern = None;
            let mut i = 4;
            while i < parts.len() {
                match parts[i].to_ascii_uppercase().as_str() {
                    "SPEED" if i + 1 < parts.len() => {
                        speed = parts[i + 1].parse().ok();
                        i += 2;
                    }
                    "PATTERN" if i + 1 < parts.len() => {
                        pattern = Some(parts[i + 1]);
                        i += 2;
                    }
                    _ => i += 1,
                }
            }
            add_pump(
                network_id,
                name,
                node1,
                node2,
                "POWER",
                None,
                power,
                speed,
                pattern,
            );
        }
        other => error!("Invalid pump type '{other}' in scenario pump '{name}'"),
    }
}

fn merge_override_to_base(network_id: i32, ov: &scenario::Override) {
    let t = ov.target_type.to_ascii_lowercase();
    let p = ov.parameter.to_ascii_lowercase();
    match (t.as_str(), p.as_str()) {
        ("junction", "demand") => {
            if let Ok(v) = ov.value.parse::<f64>() {
                Spi::run(&format!(
                    "UPDATE epanet.junctions SET demand = {v} \
                     WHERE network_id = {network_id} AND name = {}",
                    sql_text(&ov.target_id)
                ))
                .ok();
            }
        }
        ("junction", "elevation") => {
            if let Ok(v) = ov.value.parse::<f64>() {
                Spi::run(&format!(
                    "UPDATE epanet.junctions SET elevation = {v} \
                     WHERE network_id = {network_id} AND name = {}",
                    sql_text(&ov.target_id)
                ))
                .ok();
            }
        }
        ("pipe", "status") => {
            Spi::run(&format!(
                "UPDATE epanet.pipes SET status = {} \
                 WHERE network_id = {network_id} AND name = {}",
                sql_text(&ov.value),
                sql_text(&ov.target_id)
            ))
            .ok();
        }
        ("pipe", "roughness") => {
            if let Ok(v) = ov.value.parse::<f64>() {
                Spi::run(&format!(
                    "UPDATE epanet.pipes SET roughness = {v} \
                     WHERE network_id = {network_id} AND name = {}",
                    sql_text(&ov.target_id)
                ))
                .ok();
            }
        }
        _ => {}
    }
}

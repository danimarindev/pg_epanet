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
            WHERE scenario_id = {scenario_id} AND element_type = 'junction' AND name = {}
        )",
        sql_text(name)
    ))
    .unwrap()
    .unwrap_or(false)
}

fn update_junction_geom(network_id: i32, name: &str, srid: i32) {
    Spi::run(&format!(
        "UPDATE epanet.junctions j \
         SET geom = ST_SetSRID(ST_MakePoint(c.x, c.y), {srid}) \
         FROM epanet.coordinates c \
         WHERE j.network_id = c.network_id AND j.name = c.node_id \
           AND j.network_id = {network_id} AND j.name = {}",
        sql_text(name)
    ))
    .unwrap_or_else(|e| error!("SPI error updating junction geometry: {e:?}"));
}

fn update_pipe_geom(network_id: i32, name: &str, srid: i32) {
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
    Spi::run(&format!(
        "INSERT INTO epanet.scenario_elements(scenario_id, element_type, name, inp_fields, coord_x, coord_y) \
         VALUES ({scenario_id}, 'junction', {}, {}, {x}, {y})",
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

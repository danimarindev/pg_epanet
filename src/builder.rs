//! Create empty networks and add EPANET metadata elements incrementally.

use pgrx::prelude::*;

use crate::metadata;
use crate::sql_text;

/// Minimal runnable INP shell (OPTIONS + TIMES + REPORT only).
pub fn minimal_inp(name: &str) -> String {
    format!(
        "[TITLE]\n{name}\n\n\
         [OPTIONS]\n\
          Units            LPS\n\
          Headloss         H-W\n\
          Specific Gravity 1.0\n\
          Viscosity        1.0\n\
          Trials           40\n\
          Accuracy         0.001\n\
          Unbalanced       Continue 10\n\
          Pattern          1\n\
          Demand Multiplier 1.0\n\
         \n\
         [TIMES]\n\
          Duration           24:00\n\
          Hydraulic Timestep 1:00\n\
          Report Timestep    1:00\n\
         \n\
         [REPORT]\n\
          Status   No\n\
          Summary  No\n\
         \n\
         [END]\n"
    )
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

fn upsert_kv(table: &str, network_id: i32, key: &str, value: &str) {
    Spi::run(&format!(
        "INSERT INTO epanet.{table}(network_id, key, value) VALUES ({network_id}, {}, {}) \
         ON CONFLICT (network_id, key) DO UPDATE SET value = EXCLUDED.value",
        sql_text(key),
        sql_text(value)
    ))
    .unwrap_or_else(|e| error!("SPI error upserting {table}.{key}: {e:?}"));
}

/// Creates an empty network with default OPTIONS/TIMES/REPORT metadata.
pub fn create_network(name: &str, srid: i32) -> i32 {
    let inp = minimal_inp(name);
    let lit = sql_text(&inp);
    let network_id = Spi::get_one::<i32>(&format!(
        "INSERT INTO epanet.networks(name, srid, inp_text) VALUES ({}, {srid}, {lit}) RETURNING id",
        sql_text(name)
    ))
    .unwrap()
    .unwrap_or_else(|| error!("Failed to create network '{name}'"));

    metadata::import_metadata_sections(network_id, &inp);
    network_id
}

/// Adds a demand pattern from whitespace-separated multipliers.
pub fn add_pattern(network_id: i32, pattern_id: &str, multipliers: &str) {
    assert_network(network_id);
    Spi::run(&format!(
        "DELETE FROM epanet.patterns WHERE network_id = {network_id} AND pattern_id = {}",
        sql_text(pattern_id)
    ))
    .unwrap_or_else(|e| error!("SPI error clearing pattern: {e:?}"));

    let values: Vec<String> = multipliers
        .split_whitespace()
        .enumerate()
        .filter_map(|(idx, s)| {
            let mult: f64 = s.parse().ok()?;
            Some(format!(
                "({network_id},{},{idx},{mult})",
                sql_text(pattern_id)
            ))
        })
        .collect();
    if values.is_empty() {
        error!("pattern '{pattern_id}' requires at least one multiplier");
    }
    Spi::run(&format!(
        "INSERT INTO epanet.patterns(network_id, pattern_id, idx, multiplier) VALUES {}",
        values.join(",")
    ))
    .unwrap_or_else(|e| error!("SPI error inserting pattern: {e:?}"));
}

/// Adds a curve from whitespace-separated x y pairs (e.g. `0 40 10 35 20 25`).
pub fn add_curve(network_id: i32, curve_id: &str, xy_pairs: &str) {
    assert_network(network_id);
    let nums: Vec<f64> = xy_pairs
        .split_whitespace()
        .filter_map(|s| s.parse().ok())
        .collect();
    if nums.len() < 2 || nums.len() % 2 != 0 {
        error!("curve '{curve_id}' requires an even number of x y values");
    }

    Spi::run(&format!(
        "DELETE FROM epanet.curves WHERE network_id = {network_id} AND curve_id = {}",
        sql_text(curve_id)
    ))
    .unwrap_or_else(|e| error!("SPI error clearing curve: {e:?}"));

    let values: Vec<String> = nums
        .chunks(2)
        .enumerate()
        .map(|(idx, pair)| {
            format!(
                "({network_id},{},{idx},{},{})",
                sql_text(curve_id),
                pair[0],
                pair[1]
            )
        })
        .collect();
    Spi::run(&format!(
        "INSERT INTO epanet.curves(network_id, curve_id, idx, x, y) VALUES {}",
        values.join(",")
    ))
    .unwrap_or_else(|e| error!("SPI error inserting curve: {e:?}"));
}

pub fn set_option(network_id: i32, key: &str, value: &str) {
    assert_network(network_id);
    upsert_kv("options", network_id, key, value);
}

pub fn set_times(network_id: i32, key: &str, value: &str) {
    assert_network(network_id);
    upsert_kv("times", network_id, key, value);
}

pub fn set_report(network_id: i32, key: &str, value: &str) {
    assert_network(network_id);
    upsert_kv("report", network_id, key, value);
}

pub fn set_reactions(network_id: i32, key: &str, value: &str) {
    assert_network(network_id);
    upsert_kv("reactions", network_id, key, value);
}

pub fn set_quality(network_id: i32, key: &str, value: &str) {
    assert_network(network_id);
    upsert_kv("quality", network_id, key, value);
}

pub fn set_energy(network_id: i32, key: &str, value: &str) {
    assert_network(network_id);
    upsert_kv("energy", network_id, key, value);
}

pub fn add_control(network_id: i32, rule_text: &str) {
    assert_network(network_id);
    Spi::run(&format!(
        "INSERT INTO epanet.controls(network_id, idx, rule_text) \
         VALUES ({network_id}, \
         (SELECT COALESCE(MAX(idx), -1) + 1 FROM epanet.controls WHERE network_id = {network_id}), \
         {})",
        sql_text(rule_text)
    ))
    .unwrap_or_else(|e| error!("SPI error inserting control: {e:?}"));
}

pub fn add_rule(network_id: i32, rule_id: &str, rule_text: &str) {
    assert_network(network_id);
    Spi::run(&format!(
        "INSERT INTO epanet.rules(network_id, rule_id, rule_text) \
         VALUES ({network_id}, {}, {}) \
         ON CONFLICT (network_id, rule_id) DO UPDATE SET rule_text = EXCLUDED.rule_text",
        sql_text(rule_id),
        sql_text(rule_text)
    ))
    .unwrap_or_else(|e| error!("SPI error inserting rule: {e:?}"));
}

pub fn add_demand(
    network_id: i32,
    junction_id: &str,
    demand: f64,
    pattern: Option<&str>,
) {
    assert_network(network_id);
    let pat_sql = match pattern {
        Some(p) => sql_text(p),
        None => "NULL".into(),
    };
    Spi::run(&format!(
        "INSERT INTO epanet.demands(network_id, junction_id, demand, pattern) \
         VALUES ({network_id}, {}, {demand}, {pat_sql}) \
         ON CONFLICT (network_id, junction_id) DO UPDATE \
         SET demand = EXCLUDED.demand, pattern = EXCLUDED.pattern",
        sql_text(junction_id)
    ))
    .unwrap_or_else(|e| error!("SPI error inserting demand: {e:?}"));
}

pub fn add_emitter(network_id: i32, junction_id: &str, coefficient: f64) {
    assert_network(network_id);
    Spi::run(&format!(
        "INSERT INTO epanet.emitters(network_id, junction_id, coefficient) \
         VALUES ({network_id}, {}, {coefficient}) \
         ON CONFLICT (network_id, junction_id) DO UPDATE SET coefficient = EXCLUDED.coefficient",
        sql_text(junction_id)
    ))
    .unwrap_or_else(|e| error!("SPI error inserting emitter: {e:?}"));
}

pub fn set_link_status(network_id: i32, link_id: &str, status_value: &str) {
    assert_network(network_id);
    Spi::run(&format!(
        "INSERT INTO epanet.status(network_id, link_id, status_value) \
         VALUES ({network_id}, {}, {}) \
         ON CONFLICT (network_id, link_id) DO UPDATE SET status_value = EXCLUDED.status_value",
        sql_text(link_id),
        sql_text(status_value)
    ))
    .unwrap_or_else(|e| error!("SPI error inserting status: {e:?}"));
}

pub fn add_source(
    network_id: i32,
    node_id: &str,
    source_type: &str,
    quality: f64,
    pattern: Option<&str>,
) {
    assert_network(network_id);
    let pat_sql = match pattern {
        Some(p) => sql_text(p),
        None => "NULL".into(),
    };
    Spi::run(&format!(
        "INSERT INTO epanet.sources(network_id, node_id, source_type, quality, pattern) \
         VALUES ({network_id}, {}, {}, {quality}, {pat_sql}) \
         ON CONFLICT (network_id, node_id) DO UPDATE \
         SET source_type = EXCLUDED.source_type, quality = EXCLUDED.quality, \
             pattern = EXCLUDED.pattern",
        sql_text(node_id),
        sql_text(source_type)
    ))
    .unwrap_or_else(|e| error!("SPI error inserting source: {e:?}"));
}

/// Appends an intermediate vertex to a link (pipe/pump/valve).
pub fn add_vertex(network_id: i32, link_id: &str, x: f64, y: f64) {
    assert_network(network_id);
    let srid = Spi::get_one::<i32>(&format!(
        "SELECT srid FROM epanet.networks WHERE id = {network_id}"
    ))
    .unwrap()
    .unwrap_or_else(|| error!("No network found with id={network_id}"));

    Spi::run(&format!(
        "INSERT INTO epanet.vertices(network_id, link_id, idx, x, y) \
         VALUES ({network_id}, {}, \
         (SELECT COALESCE(MAX(idx), -1) + 1 FROM epanet.vertices \
          WHERE network_id = {network_id} AND link_id = {}), {x}, {y})",
        sql_text(link_id),
        sql_text(link_id)
    ))
    .unwrap_or_else(|e| error!("SPI error inserting vertex: {e:?}"));

    refresh_link_geom(network_id, link_id, srid);
}

fn refresh_link_geom(network_id: i32, link_id: &str, srid: i32) {
    let lit = sql_text(link_id);
    for table in ["pipes", "pumps", "valves"] {
        Spi::run(&format!(
            "UPDATE epanet.{table} l \
             SET geom = ST_SetSRID( \
                 ST_MakeLine( \
                     ARRAY[ST_MakePoint(c1.x, c1.y)] \
                     || ARRAY(SELECT ST_MakePoint(v.x, v.y) \
                              FROM epanet.vertices v \
                              WHERE v.network_id = l.network_id AND v.link_id = l.name \
                              ORDER BY v.idx) \
                     || ARRAY[ST_MakePoint(c2.x, c2.y)] \
                 ), {srid}) \
             FROM epanet.coordinates c1, epanet.coordinates c2 \
             WHERE l.network_id = c1.network_id AND l.node1 = c1.node_id \
               AND l.network_id = c2.network_id AND l.node2 = c2.node_id \
               AND l.network_id = {network_id} AND l.name = {lit} \
               AND EXISTS (SELECT 1 FROM epanet.vertices v \
                           WHERE v.network_id = {network_id} AND v.link_id = {lit})"
        ))
        .ok();
    }
    // Pipes always use vertex-aware geometry when vertices exist.
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
           AND p.network_id = {network_id} AND p.name = {lit}"
    ))
    .ok();
}

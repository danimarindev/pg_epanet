//! Map-editing helpers: move nodes and geometry-aware pipe shapes.

use pgrx::prelude::*;
use pgrx::spi::SpiResult;

use crate::sql_text;
use crate::topology;

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

fn node_table(network_id: i32, node_id: &str) -> Option<&'static str> {
    for table in ["junctions", "reservoirs", "tanks"] {
        let exists = Spi::get_one::<bool>(&format!(
            "SELECT EXISTS(SELECT 1 FROM epanet.{table} \
             WHERE network_id = {network_id} AND name = {})",
            sql_text(node_id)
        ))
        .unwrap()
        .unwrap_or(false);
        if exists {
            return Some(table);
        }
    }
    None
}

/// Updates node coordinates and cascades geometry refresh to all incident links.
pub fn set_node_coordinates(network_id: i32, node_id: &str, x: f64, y: f64) {
    assert_network(network_id);
    let table = node_table(network_id, node_id)
        .unwrap_or_else(|| error!("node '{node_id}' not found in network {network_id}"));
    let srid = network_srid(network_id);

    Spi::run(&format!(
        "UPDATE epanet.coordinates SET x = {x}, y = {y} \
         WHERE network_id = {network_id} AND node_id = {}",
        sql_text(node_id)
    ))
    .unwrap_or_else(|e| error!("SPI error updating coordinates: {e:?}"));

    topology::update_point_geom(table, network_id, node_id, srid);
    refresh_links_at_node(network_id, node_id, srid);
}

pub fn refresh_links_at_node(network_id: i32, node_id: &str, srid: i32) {
    let lit = sql_text(node_id);
    let mut links: Vec<(String, String)> = Vec::new();
    let _ = Spi::connect(|client| -> SpiResult<_> {
        for table in ["pipes", "pumps", "valves"] {
            let rows = client.select(
                &format!(
                    "SELECT name, '{table}' AS kind FROM epanet.{table} \
                     WHERE network_id = {network_id} AND (node1 = {lit} OR node2 = {lit})"
                ),
                None,
                None,
            )?;
            for row in rows {
                links.push((row.get_by_name("name")?.unwrap(), table.to_string()));
            }
        }
        Ok(())
    });

    for (name, kind) in links {
        match kind.as_str() {
            "pipes" => topology::update_pipe_geom(network_id, &name, srid),
            "pumps" => topology::update_direct_link_geom("pumps", network_id, &name, srid),
            "valves" => topology::update_direct_link_geom("valves", network_id, &name, srid),
            _ => {}
        }
    }
}

/// Applies a LineString geometry to an existing pipe: interior points become vertices.
pub fn apply_pipe_linestring(network_id: i32, pipe_name: &str, wkt: &str) {
    assert_network(network_id);
    let srid = network_srid(network_id);
    let lit = sql_text(pipe_name);
    let wkt_lit = sql_text(wkt);

    Spi::run(&format!(
        "DELETE FROM epanet.vertices WHERE network_id = {network_id} AND link_id = {lit}"
    ))
    .unwrap_or_else(|e| error!("SPI error clearing vertices: {e:?}"));

    Spi::run(&format!(
        "INSERT INTO epanet.vertices(network_id, link_id, idx, x, y) \
         SELECT {network_id}, {lit}, gs.idx - 2, ST_X(ST_PointN(g, gs.idx)), ST_Y(ST_PointN(g, gs.idx)) \
         FROM ( \
           SELECT ST_Transform( \
             CASE WHEN ST_SRID(ln) = 0 THEN ST_SetSRID(ln, {srid}) ELSE ln END, {srid} \
           ) AS g \
           FROM (SELECT ST_GeomFromText({wkt_lit}, {srid}) AS ln) s \
         ) q, \
         generate_series(2, ST_NPoints(q.g) - 1) AS gs(idx) \
         WHERE ST_NPoints(q.g) > 2"
    ))
    .unwrap_or_else(|e| error!("SPI error inserting vertices from linestring: {e:?}"));

    Spi::run(&format!(
        "UPDATE epanet.pipes p \
         SET geom = ST_Transform( \
           CASE WHEN ST_SRID(ln) = 0 THEN ST_SetSRID(ln, {srid}) ELSE ln END, {srid} \
         ) \
         FROM (SELECT ST_GeomFromText({wkt_lit}, {srid}) AS ln) s \
         WHERE p.network_id = {network_id} AND p.name = {lit}"
    ))
    .unwrap_or_else(|e| error!("SPI error setting pipe geometry: {e:?}"));
}

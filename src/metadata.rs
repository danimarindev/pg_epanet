//! Table-returning functions and import helpers for EPANET metadata INP sections.

use pgrx::prelude::*;

use crate::epanet_sections;
use crate::sql_text;

macro_rules! kv_table_fn {
    ($fn_name:ident, $section:literal) => {
        #[pg_extern]
        fn $fn_name(
            inp_text: &str,
        ) -> TableIterator<'static, (name!(key, String), name!(value, String))> {
            let rows = epanet_sections::parse_key_value_section(inp_text, $section);
            TableIterator::new(rows.into_iter())
        }
    };
}

#[pg_extern]
fn epanet_patterns(
    inp_text: &str,
) -> TableIterator<
    'static,
    (
        name!(pattern_id, String),
        name!(idx, i32),
        name!(multiplier, f64),
    ),
> {
    let rows = epanet_sections::parse_patterns(inp_text);
    TableIterator::new(
        rows.into_iter()
            .map(|(id, idx, mult)| (id, idx, mult)),
    )
}

#[pg_extern]
fn epanet_curves(
    inp_text: &str,
) -> TableIterator<
    'static,
    (
        name!(curve_id, String),
        name!(idx, i32),
        name!(x, f64),
        name!(y, f64),
    ),
> {
    let rows = epanet_sections::parse_curves(inp_text);
    TableIterator::new(rows.into_iter().map(|(id, idx, x, y)| (id, idx, x, y)))
}

kv_table_fn!(epanet_options, "OPTIONS");
kv_table_fn!(epanet_times, "TIMES");
kv_table_fn!(epanet_reactions, "REACTIONS");
kv_table_fn!(epanet_quality, "QUALITY");
kv_table_fn!(epanet_energy, "ENERGY");
kv_table_fn!(epanet_report, "REPORT");

#[pg_extern]
fn epanet_controls(
    inp_text: &str,
) -> TableIterator<'static, (name!(idx, i32), name!(rule_text, String))> {
    let rows = epanet_sections::parse_controls(inp_text);
    TableIterator::new(
        rows.into_iter()
            .enumerate()
            .map(|(i, text)| (i as i32, text)),
    )
}

#[pg_extern]
fn epanet_rules(
    inp_text: &str,
) -> TableIterator<'static, (name!(rule_id, String), name!(rule_text, String))> {
    let rows = epanet_sections::parse_rules(inp_text);
    TableIterator::new(rows.into_iter())
}

#[pg_extern]
fn epanet_demands(
    inp_text: &str,
) -> TableIterator<
    'static,
    (
        name!(junction_id, String),
        name!(demand, f64),
        name!(pattern, Option<String>),
    ),
> {
    let rows = epanet_sections::parse_demands(inp_text);
    TableIterator::new(rows.into_iter())
}

#[pg_extern]
fn epanet_emitters(
    inp_text: &str,
) -> TableIterator<'static, (name!(junction_id, String), name!(coefficient, f64))> {
    let rows = epanet_sections::parse_emitters(inp_text);
    TableIterator::new(rows.into_iter())
}

#[pg_extern]
fn epanet_status(
    inp_text: &str,
) -> TableIterator<'static, (name!(link_id, String), name!(status_value, String))> {
    let rows = epanet_sections::parse_status(inp_text);
    TableIterator::new(rows.into_iter())
}

#[pg_extern]
fn epanet_sources(
    inp_text: &str,
) -> TableIterator<
    'static,
    (
        name!(node_id, String),
        name!(source_type, String),
        name!(quality, f64),
        name!(pattern, Option<String>),
    ),
> {
    let rows = epanet_sections::parse_sources(inp_text);
    TableIterator::new(rows.into_iter())
}

pub fn import_metadata_sections(network_id: i32, inp_text: &str) {
    let lit = sql_text(inp_text);

    for (table, func, cols) in [
        ("patterns", "epanet_patterns", "pattern_id, idx, multiplier"),
        ("curves", "epanet_curves", "curve_id, idx, x, y"),
        ("options", "epanet_options", "key, value"),
        ("times", "epanet_times", "key, value"),
        ("reactions", "epanet_reactions", "key, value"),
        ("quality", "epanet_quality", "key, value"),
        ("energy", "epanet_energy", "key, value"),
        ("report", "epanet_report", "key, value"),
        ("demands", "epanet_demands", "junction_id, demand, pattern"),
        ("emitters", "epanet_emitters", "junction_id, coefficient"),
        ("status", "epanet_status", "link_id, status_value"),
        ("sources", "epanet_sources", "node_id, source_type, quality, pattern"),
    ] {
        Spi::run(&format!(
            "INSERT INTO epanet.{table}(network_id, {cols}) \
             SELECT {network_id}, * FROM {func}({lit})"
        ))
        .unwrap();
    }

    Spi::run(&format!(
        "INSERT INTO epanet.controls(network_id, idx, rule_text) \
         SELECT {network_id}, idx, rule_text FROM epanet_controls({lit})"
    ))
    .unwrap();

    Spi::run(&format!(
        "INSERT INTO epanet.rules(network_id, rule_id, rule_text) \
         SELECT {network_id}, rule_id, rule_text FROM epanet_rules({lit})"
    ))
    .unwrap();
}

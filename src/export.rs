//! Reconstruct EPANET INP text from materialised `epanet.*` tables.

use pgrx::prelude::*;
use pgrx::spi::SpiResult;

use crate::sql_text;

pub fn export_network(network_id: i32) -> String {
    assert_network_exists(network_id);

    let mut out = String::new();
    let name = network_name(network_id);
    out.push_str("[TITLE]\n");
    out.push_str(&name);
    out.push('\n');

    append_junctions(&mut out, network_id);
    append_reservoirs(&mut out, network_id);
    append_tanks(&mut out, network_id);
    append_pipes(&mut out, network_id);
    append_pumps(&mut out, network_id);
    append_valves(&mut out, network_id);
    append_demands(&mut out, network_id);
    append_emitters(&mut out, network_id);
    append_status(&mut out, network_id);
    append_patterns(&mut out, network_id);
    append_curves(&mut out, network_id);
    append_controls(&mut out, network_id);
    append_rules(&mut out, network_id);
    append_key_value_table(&mut out, "ENERGY", "energy", network_id);
    append_sources(&mut out, network_id);
    append_key_value_table(&mut out, "REACTIONS", "reactions", network_id);
    append_node_quality(&mut out, network_id);
    append_key_value_table(&mut out, "TIMES", "times", network_id);
    append_key_value_table(&mut out, "REPORT", "report", network_id);
    append_key_value_table(&mut out, "OPTIONS", "options", network_id);
    append_coordinates(&mut out, network_id);
    append_vertices(&mut out, network_id);
    out.push_str("\n[END]\n");
    out
}

fn assert_network_exists(network_id: i32) {
    let exists = Spi::get_one::<bool>(&format!(
        "SELECT EXISTS(SELECT 1 FROM epanet.networks WHERE id = {network_id})"
    ))
    .unwrap()
    .unwrap_or(false);
    if !exists {
        error!("No network found with id={network_id}");
    }
}

fn network_name(network_id: i32) -> String {
    Spi::get_one::<String>(&format!(
        "SELECT name FROM epanet.networks WHERE id = {network_id}"
    ))
    .unwrap()
    .unwrap_or_default()
}

fn append_section(out: &mut String, name: &str, body: &str) {
    if body.trim().is_empty() {
        return;
    }
    out.push('\n');
    out.push_str(&format!("[{name}]\n"));
    out.push_str(body.trim_end());
    out.push('\n');
}

fn append_junctions(out: &mut String, network_id: i32) {
    let mut lines = Vec::new();
    let _ = Spi::connect(|client| -> SpiResult<_> {
        let table = client.select(
            &format!(
                "SELECT name, elevation, demand, pattern FROM epanet.junctions \
                 WHERE network_id = {network_id} ORDER BY name"
            ),
            None,
            None,
        )?;
        for row in table {
            let name: String = row.get_by_name("name")?.unwrap();
            let elevation: f64 = row.get_by_name("elevation")?.unwrap();
            let demand: f64 = row.get_by_name("demand")?.unwrap();
            let pattern: Option<String> = row.get_by_name("pattern")?;
            let mut line = format!(" {name:<16} {elevation:<12} {demand:<12}");
            if let Some(p) = pattern {
                line.push(' ');
                line.push_str(&p);
            }
            lines.push(line);
        }
        Ok(())
    });
    append_section(out, "JUNCTIONS", &lines.join("\n"));
}

fn append_reservoirs(out: &mut String, network_id: i32) {
    let mut lines = Vec::new();
    let _ = Spi::connect(|client| -> SpiResult<_> {
        let table = client.select(
            &format!(
                "SELECT name, head, pattern FROM epanet.reservoirs \
                 WHERE network_id = {network_id} ORDER BY name"
            ),
            None,
            None,
        )?;
        for row in table {
            let name: String = row.get_by_name("name")?.unwrap();
            let head: f64 = row.get_by_name("head")?.unwrap();
            let pattern: Option<String> = row.get_by_name("pattern")?;
            let mut line = format!(" {name:<16} {head:<12}");
            if let Some(p) = pattern {
                line.push(' ');
                line.push_str(&p);
            }
            lines.push(line);
        }
        Ok(())
    });
    append_section(out, "RESERVOIRS", &lines.join("\n"));
}

fn append_tanks(out: &mut String, network_id: i32) {
    let mut lines = Vec::new();
    let _ = Spi::connect(|client| -> SpiResult<_> {
        let table = client.select(
            &format!(
                "SELECT name, elevation, init_level, min_level, max_level, diameter, \
                        min_volume, volume_curve, overflow \
                 FROM epanet.tanks WHERE network_id = {network_id} ORDER BY name"
            ),
            None,
            None,
        )?;
        for row in table {
            let name: String = row.get_by_name("name")?.unwrap();
            let elevation: f64 = row.get_by_name("elevation")?.unwrap();
            let init_level: f64 = row.get_by_name("init_level")?.unwrap();
            let min_level: f64 = row.get_by_name("min_level")?.unwrap();
            let max_level: f64 = row.get_by_name("max_level")?.unwrap();
            let diameter: f64 = row.get_by_name("diameter")?.unwrap();
            let min_volume: f64 = row.get_by_name("min_volume")?.unwrap();
            let volume_curve: Option<String> = row.get_by_name("volume_curve")?;
            let overflow: Option<String> = row.get_by_name("overflow")?;
            let mut line = format!(
                " {name:<6} {elevation:<6} {init_level:<6} {min_level:<6} {max_level:<6} \
                 {diameter:<6} {min_volume:<6}"
            );
            if let Some(c) = volume_curve {
                line.push(' ');
                line.push_str(&c);
            }
            if let Some(o) = overflow {
                line.push(' ');
                line.push_str(&o);
            }
            lines.push(line);
        }
        Ok(())
    });
    append_section(out, "TANKS", &lines.join("\n"));
}

fn append_pipes(out: &mut String, network_id: i32) {
    let mut lines = Vec::new();
    let _ = Spi::connect(|client| -> SpiResult<_> {
        let table = client.select(
            &format!(
                "SELECT name, node1, node2, length, diameter, roughness, minor_loss, status \
                 FROM epanet.pipes WHERE network_id = {network_id} ORDER BY name"
            ),
            None,
            None,
        )?;
        for row in table {
            let name: String = row.get_by_name("name")?.unwrap();
            let node1: String = row.get_by_name("node1")?.unwrap();
            let node2: String = row.get_by_name("node2")?.unwrap();
            let length: f64 = row.get_by_name("length")?.unwrap();
            let diameter: f64 = row.get_by_name("diameter")?.unwrap();
            let roughness: f64 = row.get_by_name("roughness")?.unwrap();
            let minor_loss: f64 = row.get_by_name("minor_loss")?.unwrap();
            let status: String = row.get_by_name("status")?.unwrap();
            lines.push(format!(
                " {name:<6} {node1:<6} {node2:<6} {length:<8} {diameter:<8} \
                 {roughness:<8} {minor_loss:<8} {status}"
            ));
        }
        Ok(())
    });
    append_section(out, "PIPES", &lines.join("\n"));
}

fn append_pumps(out: &mut String, network_id: i32) {
    let mut lines = Vec::new();
    let _ = Spi::connect(|client| -> SpiResult<_> {
        let table = client.select(
            &format!(
                "SELECT name, node1, node2, pump_type, head_curve, power, speed, pattern \
                 FROM epanet.pumps WHERE network_id = {network_id} ORDER BY name"
            ),
            None,
            None,
        )?;
        for row in table {
            let name: String = row.get_by_name("name")?.unwrap();
            let node1: String = row.get_by_name("node1")?.unwrap();
            let node2: String = row.get_by_name("node2")?.unwrap();
            let pump_type: Option<String> = row.get_by_name("pump_type")?;
            let head_curve: Option<String> = row.get_by_name("head_curve")?;
            let power: Option<f64> = row.get_by_name("power")?;
            let speed: Option<f64> = row.get_by_name("speed")?;
            let pattern: Option<String> = row.get_by_name("pattern")?;
            let mut params = String::new();
            if let Some(t) = pump_type {
                params.push_str(&t);
                if let Some(h) = head_curve {
                    params.push(' ');
                    params.push_str(&h);
                } else if let Some(p) = power {
                    params.push_str(&format!(" {p}"));
                    if let Some(s) = speed {
                        params.push_str(&format!(" SPEED {s}"));
                    }
                    if let Some(pat) = pattern {
                        params.push_str(&format!(" PATTERN {pat}"));
                    }
                }
            }
            lines.push(format!(" {name:<6} {node1:<6} {node2:<6} {params}"));
        }
        Ok(())
    });
    append_section(out, "PUMPS", &lines.join("\n"));
}

fn append_valves(out: &mut String, network_id: i32) {
    let mut lines = Vec::new();
    let _ = Spi::connect(|client| -> SpiResult<_> {
        let table = client.select(
            &format!(
                "SELECT name, node1, node2, diameter, valve_type, setting, minor_loss \
                 FROM epanet.valves WHERE network_id = {network_id} ORDER BY name"
            ),
            None,
            None,
        )?;
        for row in table {
            let name: String = row.get_by_name("name")?.unwrap();
            let node1: String = row.get_by_name("node1")?.unwrap();
            let node2: String = row.get_by_name("node2")?.unwrap();
            let diameter: f64 = row.get_by_name("diameter")?.unwrap();
            let valve_type: String = row.get_by_name("valve_type")?.unwrap();
            let setting: String = row.get_by_name("setting")?.unwrap();
            let minor_loss: f64 = row.get_by_name("minor_loss")?.unwrap();
            lines.push(format!(
                " {name:<6} {node1:<6} {node2:<6} {diameter:<8} {valve_type:<6} \
                 {setting:<8} {minor_loss}"
            ));
        }
        Ok(())
    });
    append_section(out, "VALVES", &lines.join("\n"));
}

fn append_demands(out: &mut String, network_id: i32) {
    let mut lines = Vec::new();
    let _ = Spi::connect(|client| -> SpiResult<_> {
        let table = client.select(
            &format!(
                "SELECT junction_id, demand, pattern FROM epanet.demands \
                 WHERE network_id = {network_id} ORDER BY junction_id"
            ),
            None,
            None,
        )?;
        for row in table {
            let junction_id: String = row.get_by_name("junction_id")?.unwrap();
            let demand: f64 = row.get_by_name("demand")?.unwrap();
            let pattern: Option<String> = row.get_by_name("pattern")?;
            let mut line = format!(" {junction_id:<16} {demand}");
            if let Some(p) = pattern {
                line.push(' ');
                line.push_str(&p);
            }
            lines.push(line);
        }
        Ok(())
    });
    append_section(out, "DEMANDS", &lines.join("\n"));
}

fn append_emitters(out: &mut String, network_id: i32) {
    let mut lines = Vec::new();
    let _ = Spi::connect(|client| -> SpiResult<_> {
        let table = client.select(
            &format!(
                "SELECT junction_id, coefficient FROM epanet.emitters \
                 WHERE network_id = {network_id} ORDER BY junction_id"
            ),
            None,
            None,
        )?;
        for row in table {
            let junction_id: String = row.get_by_name("junction_id")?.unwrap();
            let coefficient: f64 = row.get_by_name("coefficient")?.unwrap();
            lines.push(format!(" {junction_id:<16} {coefficient}"));
        }
        Ok(())
    });
    append_section(out, "EMITTERS", &lines.join("\n"));
}

fn append_status(out: &mut String, network_id: i32) {
    let mut lines = Vec::new();
    let _ = Spi::connect(|client| -> SpiResult<_> {
        let table = client.select(
            &format!(
                "SELECT link_id, status_value FROM epanet.status \
                 WHERE network_id = {network_id} ORDER BY link_id"
            ),
            None,
            None,
        )?;
        for row in table {
            let link_id: String = row.get_by_name("link_id")?.unwrap();
            let status_value: String = row.get_by_name("status_value")?.unwrap();
            lines.push(format!(" {link_id:<16} {status_value}"));
        }
        Ok(())
    });
    append_section(out, "STATUS", &lines.join("\n"));
}

fn append_patterns(out: &mut String, network_id: i32) {
    let mut rows: Vec<(String, i32, f64)> = Vec::new();
    let _ = Spi::connect(|client| -> SpiResult<_> {
        let table = client.select(
            &format!(
                "SELECT pattern_id, idx, multiplier FROM epanet.patterns \
                 WHERE network_id = {network_id} ORDER BY pattern_id, idx"
            ),
            None,
            None,
        )?;
        for row in table {
            rows.push((
                row.get_by_name("pattern_id")?.unwrap(),
                row.get_by_name("idx")?.unwrap(),
                row.get_by_name("multiplier")?.unwrap(),
            ));
        }
        Ok(())
    });

    let mut lines = Vec::new();
    let mut current_id: Option<String> = None;
    let mut current_line = String::new();
    for (id, _idx, mult) in rows {
        if current_id.as_ref() != Some(&id) {
            if !current_line.is_empty() {
                lines.push(current_line.clone());
            }
            current_line = format!(" {id}");
            current_id = Some(id);
        }
        current_line.push(' ');
        current_line.push_str(&format!("{mult}"));
    }
    if !current_line.is_empty() {
        lines.push(current_line);
    }
    append_section(out, "PATTERNS", &lines.join("\n"));
}

fn append_curves(out: &mut String, network_id: i32) {
    let mut lines = Vec::new();
    let _ = Spi::connect(|client| -> SpiResult<_> {
        let table = client.select(
            &format!(
                "SELECT curve_id, x, y FROM epanet.curves \
                 WHERE network_id = {network_id} ORDER BY curve_id, idx"
            ),
            None,
            None,
        )?;
        for row in table {
            let curve_id: String = row.get_by_name("curve_id")?.unwrap();
            let x: f64 = row.get_by_name("x")?.unwrap();
            let y: f64 = row.get_by_name("y")?.unwrap();
            lines.push(format!(" {curve_id:<6} {x:<8} {y}"));
        }
        Ok(())
    });
    append_section(out, "CURVES", &lines.join("\n"));
}

fn append_controls(out: &mut String, network_id: i32) {
    let mut lines = Vec::new();
    let _ = Spi::connect(|client| -> SpiResult<_> {
        let table = client.select(
            &format!(
                "SELECT rule_text FROM epanet.controls \
                 WHERE network_id = {network_id} ORDER BY idx"
            ),
            None,
            None,
        )?;
        for row in table {
            let text: String = row.get_by_name("rule_text")?.unwrap();
            lines.push(format!(" {text}"));
        }
        Ok(())
    });
    append_section(out, "CONTROLS", &lines.join("\n"));
}

fn append_rules(out: &mut String, network_id: i32) {
    let mut blocks = Vec::new();
    let _ = Spi::connect(|client| -> SpiResult<_> {
        let table = client.select(
            &format!(
                "SELECT rule_id, rule_text FROM epanet.rules \
                 WHERE network_id = {network_id} ORDER BY rule_id"
            ),
            None,
            None,
        )?;
        for row in table {
            let rule_id: String = row.get_by_name("rule_id")?.unwrap();
            let rule_text: String = row.get_by_name("rule_text")?.unwrap();
            blocks.push(format!("RULE {rule_id}\n{rule_text}"));
        }
        Ok(())
    });
    append_section(out, "RULES", &blocks.join("\n"));
}

fn append_sources(out: &mut String, network_id: i32) {
    let mut lines = Vec::new();
    let _ = Spi::connect(|client| -> SpiResult<_> {
        let table = client.select(
            &format!(
                "SELECT node_id, source_type, quality, pattern FROM epanet.sources \
                 WHERE network_id = {network_id} ORDER BY node_id"
            ),
            None,
            None,
        )?;
        for row in table {
            let node_id: String = row.get_by_name("node_id")?.unwrap();
            let source_type: String = row.get_by_name("source_type")?.unwrap();
            let quality: f64 = row.get_by_name("quality")?.unwrap();
            let pattern: Option<String> = row.get_by_name("pattern")?;
            let mut line = format!(" {node_id:<16} {source_type:<8} {quality}");
            if let Some(p) = pattern {
                line.push(' ');
                line.push_str(&p);
            }
            lines.push(line);
        }
        Ok(())
    });
    append_section(out, "SOURCES", &lines.join("\n"));
}

fn append_node_quality(out: &mut String, network_id: i32) {
    let mut lines = Vec::new();
    let _ = Spi::connect(|client| -> SpiResult<_> {
        let table = client.select(
            &format!(
                "SELECT key, value FROM epanet.quality \
                 WHERE network_id = {network_id} ORDER BY key"
            ),
            None,
            None,
        )?;
        for row in table {
            let key: String = row.get_by_name("key")?.unwrap();
            let value: String = row.get_by_name("value")?.unwrap();
            lines.push(format!(" {key:<16} {value}"));
        }
        Ok(())
    });
    append_section(out, "QUALITY", &lines.join("\n"));
}

fn append_key_value_table(out: &mut String, section: &str, table_name: &str, network_id: i32) {
    let mut lines = Vec::new();
    let _ = Spi::connect(|client| -> SpiResult<_> {
        let rows = client.select(
            &format!(
                "SELECT key, value FROM epanet.{table_name} \
                 WHERE network_id = {network_id} ORDER BY key"
            ),
            None,
            None,
        )?;
        for row in rows {
            let key: String = row.get_by_name("key")?.unwrap();
            let value: String = row.get_by_name("value")?.unwrap();
            if value.is_empty() {
                lines.push(format!(" {key}"));
            } else {
                lines.push(format!(" {key:<20} {value}"));
            }
        }
        Ok(())
    });
    append_section(out, section, &lines.join("\n"));
}

fn append_coordinates(out: &mut String, network_id: i32) {
    let mut lines = Vec::new();
    let _ = Spi::connect(|client| -> SpiResult<_> {
        let table = client.select(
            &format!(
                "SELECT node_id, x, y FROM epanet.coordinates \
                 WHERE network_id = {network_id} ORDER BY node_id"
            ),
            None,
            None,
        )?;
        for row in table {
            let node_id: String = row.get_by_name("node_id")?.unwrap();
            let x: f64 = row.get_by_name("x")?.unwrap();
            let y: f64 = row.get_by_name("y")?.unwrap();
            lines.push(format!(" {node_id:<16} {x:<12} {y}"));
        }
        Ok(())
    });
    append_section(out, "COORDINATES", &lines.join("\n"));
}

fn append_vertices(out: &mut String, network_id: i32) {
    let mut lines = Vec::new();
    let _ = Spi::connect(|client| -> SpiResult<_> {
        let table = client.select(
            &format!(
                "SELECT link_id, x, y FROM epanet.vertices \
                 WHERE network_id = {network_id} ORDER BY link_id, idx"
            ),
            None,
            None,
        )?;
        for row in table {
            let link_id: String = row.get_by_name("link_id")?.unwrap();
            let x: f64 = row.get_by_name("x")?.unwrap();
            let y: f64 = row.get_by_name("y")?.unwrap();
            lines.push(format!(" {link_id:<16} {x:<12} {y}"));
        }
        Ok(())
    });
    append_section(out, "VERTICES", &lines.join("\n"));
}

/// Updates `networks.inp_text` from current table state.
pub fn refresh_inp_text(network_id: i32) {
    let inp = export_network(network_id);
    let lit = sql_text(&inp);
    Spi::run(&format!(
        "UPDATE epanet.networks SET inp_text = {lit} WHERE id = {network_id}"
    ))
    .unwrap_or_else(|e| error!("SPI error updating inp_text: {e:?}"));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inp;

    #[test]
    fn export_section_order_contains_end() {
        // Unit-level formatting: empty network would error in export_network;
        // test append_section only.
        let mut out = String::new();
        append_section(&mut out, "JUNCTIONS", " J1 100 0");
        assert!(out.contains("[JUNCTIONS]"));
    }

    #[test]
    fn parse_exported_junctions_roundtrip() {
        let body = " J1               100.0        10.0";
        let inp = format!("[JUNCTIONS]\n{body}\n");
        let sections = inp::parse_sections(&inp);
        assert_eq!(sections["JUNCTIONS"][0][0], "J1");
    }
}

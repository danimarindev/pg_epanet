//! Scenario overlays: apply parameter changes to a base INP without mutating the network.

use pgrx::prelude::*;
use pgrx::spi::SpiResult;
use std::collections::HashMap;

use crate::inp;

#[derive(Clone, Debug)]
pub struct Override {
    pub target_type: String,
    pub target_id: String,
    pub parameter: String,
    pub value: String,
}

pub fn load_scenario(scenario_id: i32) -> (i32, f64, Vec<Override>) {
    let (network_id, demand_multiplier) = Spi::get_two::<i32, f64>(&format!(
        "SELECT network_id, demand_multiplier FROM epanet.scenarios WHERE id = {scenario_id}"
    ))
    .unwrap_or_else(|e| error!("SPI error loading scenario: {e:?}"));

    let network_id = network_id.unwrap_or_else(|| error!("No scenario found with id={scenario_id}"));
    let demand_multiplier = demand_multiplier.unwrap_or(1.0);

    let mut overrides = Vec::new();
    let _ = Spi::connect(|client| -> SpiResult<_> {
        let rows = client.select(
            &format!(
                "SELECT target_type, target_id, parameter, value \
                 FROM epanet.scenario_overrides WHERE scenario_id = {scenario_id} \
                 ORDER BY target_type, target_id, parameter"
            ),
            None,
            None,
        )?;
        for row in rows {
            overrides.push(Override {
                target_type: row.get_by_name("target_type")?.unwrap(),
                target_id: row.get_by_name("target_id")?.unwrap(),
                parameter: row.get_by_name("parameter")?.unwrap(),
                value: row.get_by_name("value")?.unwrap(),
            });
        }
        Ok(())
    });

    (network_id, demand_multiplier, overrides)
}

/// Applies scenario overrides to a base INP string (in memory only — base network is unchanged).
pub fn apply_scenario(
    base_inp: &str,
    demand_multiplier: f64,
    overrides: &[Override],
    scenario_id: Option<i32>,
) -> String {
    let mut sections = inp::parse_sections(base_inp);

    if (demand_multiplier - 1.0).abs() > f64::EPSILON {
        apply_global_demand_multiplier(&mut sections, demand_multiplier);
    }

    for ov in overrides {
        apply_override(&mut sections, ov);
    }

    if let Some(sid) = scenario_id {
        apply_scenario_elements(&mut sections, sid);
    }

    inp::render_sections(&sections)
}

fn apply_scenario_elements(sections: &mut HashMap<String, Vec<Vec<String>>>, scenario_id: i32) {
    let _ = Spi::connect(|client| -> SpiResult<_> {
        let rows = client.select(
            &format!(
                "SELECT element_type, name, inp_fields, coord_x, coord_y \
                 FROM epanet.scenario_elements WHERE scenario_id = {scenario_id} \
                 ORDER BY element_type, name"
            ),
            None,
            None,
        )?;
        for row in rows {
            let etype: String = row.get_by_name("element_type")?.unwrap();
            let name: String = row.get_by_name("name")?.unwrap();
            let fields_str: String = row.get_by_name("inp_fields")?.unwrap();
            let cx: Option<f64> = row.get_by_name("coord_x")?;
            let cy: Option<f64> = row.get_by_name("coord_y")?;

            let section = match etype.as_str() {
                "junction" => "JUNCTIONS",
                "pipe" => "PIPES",
                "pump" => "PUMPS",
                "valve" => "VALVES",
                "tank" => "TANKS",
                "reservoir" => "RESERVOIRS",
                other => {
                    warning!("epanet: unknown scenario element type '{other}'");
                    continue;
                }
            };

            let fields: Vec<String> = std::iter::once(name.clone())
                .chain(fields_str.split_whitespace().map(str::to_owned))
                .collect();
            sections.entry(section.into()).or_default().push(fields);

            if let (Some(x), Some(y)) = (cx, cy) {
                sections
                    .entry("COORDINATES".into())
                    .or_default()
                    .push(vec![name, format!("{x}"), format!("{y}")]);
            }
        }
        Ok(())
    });

    let _ = Spi::connect(|client| -> SpiResult<_> {
        let rows = client.select(
            &format!(
                "SELECT link_id, x, y FROM epanet.scenario_element_vertices \
                 WHERE scenario_id = {scenario_id} ORDER BY link_id, idx"
            ),
            None,
            None,
        )?;
        for row in rows {
            let link_id: String = row.get_by_name("link_id")?.unwrap();
            let x: f64 = row.get_by_name("x")?.unwrap();
            let y: f64 = row.get_by_name("y")?.unwrap();
            sections
                .entry("VERTICES".into())
                .or_default()
                .push(vec![link_id, format!("{x}"), format!("{y}")]);
        }
        Ok(())
    });
}

fn apply_global_demand_multiplier(sections: &mut HashMap<String, Vec<Vec<String>>>, mult: f64) {
    if let Some(rows) = sections.get_mut("JUNCTIONS") {
        for fields in rows.iter_mut() {
            if fields.len() >= 3 {
                if let Ok(d) = fields[2].parse::<f64>() {
                    fields[2] = format!("{}", d * mult);
                }
            }
        }
    }
    merge_option(sections, "Demand Multiplier", &format!("{mult}"));
}

fn apply_override(sections: &mut HashMap<String, Vec<Vec<String>>>, ov: &Override) {
    let t = ov.target_type.to_ascii_lowercase();
    let p = ov.parameter.to_ascii_lowercase();

    match (t.as_str(), p.as_str()) {
        ("junction", "demand") => set_junction_demand(sections, &ov.target_id, &ov.value),
        ("junction", "elevation") => set_junction_field(sections, &ov.target_id, 1, &ov.value),
        ("pipe", "status") => set_link_status(sections, "PIPES", &ov.target_id, &ov.value),
        ("pipe", "roughness") => set_pipe_field(sections, &ov.target_id, 5, &ov.value),
        ("pipe", "roughness_factor") => multiply_pipe_field(sections, &ov.target_id, 5, &ov.value),
        ("pump", "status") => set_link_status(sections, "PUMPS", &ov.target_id, &ov.value),
        ("pump", "speed") => set_pump_speed(sections, &ov.target_id, &ov.value),
        ("valve", "status") => set_link_status(sections, "VALVES", &ov.target_id, &ov.value),
        ("valve", "setting") => set_valve_setting(sections, &ov.target_id, &ov.value),
        ("option", _) => merge_option(sections, &ov.target_id, &ov.value),
        ("status", "value") | ("link", "status") => {
            append_status(sections, &ov.target_id, &ov.value)
        }
        ("demand", "value") => set_demand_row(sections, &ov.target_id, &ov.value, None),
        ("demand", "pattern") => set_demand_row(sections, &ov.target_id, "0", Some(&ov.value)),
        ("emitter", "coefficient") => set_emitter(sections, &ov.target_id, &ov.value),
        _ => warning!(
            "epanet: unknown scenario override {}.{}.{} — skipped",
            ov.target_type,
            ov.target_id,
            ov.parameter
        ),
    }
}

fn find_row<'a>(rows: &'a mut [Vec<String>], id: &str) -> Option<&'a mut Vec<String>> {
    rows.iter_mut().find(|f| f.first().map(|s| s.as_str()) == Some(id))
}

fn set_junction_demand(sections: &mut HashMap<String, Vec<Vec<String>>>, id: &str, demand: &str) {
    if let Some(rows) = sections.get_mut("JUNCTIONS") {
        if let Some(fields) = find_row(rows, id) {
            while fields.len() < 3 {
                fields.push("0".into());
            }
            fields[2] = demand.to_string();
        }
    }
    set_demand_row(sections, id, demand, None);
}

fn set_junction_field(
    sections: &mut HashMap<String, Vec<Vec<String>>>,
    id: &str,
    idx: usize,
    value: &str,
) {
    if let Some(rows) = sections.get_mut("JUNCTIONS") {
        if let Some(fields) = find_row(rows, id) {
            while fields.len() <= idx {
                fields.push("0".into());
            }
            fields[idx] = value.to_string();
        }
    }
}

fn set_pipe_field(sections: &mut HashMap<String, Vec<Vec<String>>>, id: &str, idx: usize, value: &str) {
    if let Some(rows) = sections.get_mut("PIPES") {
        if let Some(fields) = find_row(rows, id) {
            while fields.len() <= idx {
                fields.push("0".into());
            }
            fields[idx] = value.to_string();
        }
    }
}

fn multiply_pipe_field(
    sections: &mut HashMap<String, Vec<Vec<String>>>,
    id: &str,
    idx: usize,
    factor: &str,
) {
    let Ok(f) = factor.parse::<f64>() else { return };
    if let Some(rows) = sections.get_mut("PIPES") {
        if let Some(fields) = find_row(rows, id) {
            while fields.len() <= idx {
                fields.push("100".into());
            }
            if let Ok(v) = fields[idx].parse::<f64>() {
                fields[idx] = format!("{}", v * f);
            }
        }
    }
}

fn set_link_status(
    sections: &mut HashMap<String, Vec<Vec<String>>>,
    section: &str,
    id: &str,
    status: &str,
) {
    if section == "PIPES" {
        if let Some(rows) = sections.get_mut("PIPES") {
            if let Some(fields) = find_row(rows, id) {
                while fields.len() < 8 {
                    fields.push(if fields.len() == 7 {
                        "Open".into()
                    } else {
                        "0".into()
                    });
                }
                fields[7] = status.to_string();
            }
        }
    }
    append_status(sections, id, status);
}

fn set_pump_speed(sections: &mut HashMap<String, Vec<Vec<String>>>, id: &str, speed: &str) {
    if let Some(rows) = sections.get_mut("PUMPS") {
        if let Some(fields) = find_row(rows, id) {
            // Rebuild HEAD curve line or POWER line with SPEED token
            if fields.len() >= 4 {
                let params: Vec<String> = fields[3..].to_vec();
                let mut out = params;
                if let Some(pos) = out.iter().position(|s| s.eq_ignore_ascii_case("SPEED")) {
                    if pos + 1 < out.len() {
                        out[pos + 1] = speed.to_string();
                    }
                } else {
                    out.push("SPEED".into());
                    out.push(speed.to_string());
                }
                fields.truncate(3);
                fields.extend(out);
            }
        }
    }
}

fn set_valve_setting(sections: &mut HashMap<String, Vec<Vec<String>>>, id: &str, setting: &str) {
    if let Some(rows) = sections.get_mut("VALVES") {
        if let Some(fields) = find_row(rows, id) {
            while fields.len() < 6 {
                fields.push("0".into());
            }
            fields[5] = setting.to_string();
        }
    }
}

fn merge_option(sections: &mut HashMap<String, Vec<Vec<String>>>, key: &str, value: &str) {
    let rows = sections.entry("OPTIONS".into()).or_default();
    if let Some(fields) = rows.iter_mut().find(|f| f.first().map(|s| s.as_str()) == Some(key)) {
        if fields.len() >= 2 {
            fields[1..] = value.split_whitespace().map(str::to_owned).collect();
        } else {
            *fields = vec![key.to_string(), value.to_string()];
        }
        return;
    }
    // Two-word keys like "Demand Multiplier"
    if key.contains(' ') {
        let parts: Vec<&str> = key.splitn(2, ' ').collect();
        rows.push(vec![
            parts[0].to_string(),
            parts[1].to_string(),
            value.to_string(),
        ]);
    } else {
        rows.push(vec![key.to_string(), value.to_string()]);
    }
}

fn append_status(sections: &mut HashMap<String, Vec<Vec<String>>>, link_id: &str, status: &str) {
    let rows = sections.entry("STATUS".into()).or_default();
    if let Some(fields) = find_row(rows, link_id) {
        fields[1..] = status.split_whitespace().map(str::to_owned).collect();
    } else {
        rows.push(vec![link_id.to_string(), status.to_string()]);
    }
}

fn set_demand_row(
    sections: &mut HashMap<String, Vec<Vec<String>>>,
    junction_id: &str,
    demand: &str,
    pattern: Option<&str>,
) {
    let rows = sections.entry("DEMANDS".into()).or_default();
    if let Some(fields) = find_row(rows, junction_id) {
        fields[1] = demand.to_string();
        if let Some(p) = pattern {
            if fields.len() >= 3 {
                fields[2] = p.to_string();
            } else {
                fields.push(p.to_string());
            }
        }
    } else {
        let mut row = vec![junction_id.to_string(), demand.to_string()];
        if let Some(p) = pattern {
            row.push(p.to_string());
        }
        rows.push(row);
    }
}

fn set_emitter(sections: &mut HashMap<String, Vec<Vec<String>>>, junction_id: &str, coef: &str) {
    let rows = sections.entry("EMITTERS".into()).or_default();
    if let Some(fields) = find_row(rows, junction_id) {
        fields[1] = coef.to_string();
    } else {
        rows.push(vec![junction_id.to_string(), coef.to_string()]);
    }
}

pub fn effective_inp_for_scenario(scenario_id: i32) -> (i32, String) {
    let (network_id, demand_multiplier, overrides) = load_scenario(scenario_id);
    let base = Spi::get_one::<String>(&format!(
        "SELECT inp_text FROM epanet.networks WHERE id = {network_id}"
    ))
    .unwrap()
    .unwrap_or_else(|| error!("No network found for scenario {scenario_id}"));
    let inp = apply_scenario(&base, demand_multiplier, &overrides, Some(scenario_id));
    (network_id, inp)
}

#[cfg(test)]
mod tests {
    use super::*;

    const BASE: &str = r#"[JUNCTIONS]
J1 100.0 10.0
J2 90.0 5.0
[PIPES]
P1 J1 J2 100.0 200.0 100.0 0.0 Open
[OPTIONS]
Units LPS
Demand Multiplier 1.0
"#;

    #[test]
    fn apply_junction_demand_override() {
        let out = apply_scenario(
            BASE,
            1.0,
            &[Override {
                target_type: "junction".into(),
                target_id: "J1".into(),
                parameter: "demand".into(),
                value: "999".into(),
            }],
            None,
        );
        assert!(out.contains("999"));
        assert!(!BASE.contains("999"));
    }

    #[test]
    fn apply_pipe_closure() {
        let out = apply_scenario(
            BASE,
            1.0,
            &[Override {
                target_type: "pipe".into(),
                target_id: "P1".into(),
                parameter: "status".into(),
                value: "Closed".into(),
            }],
            None,
        );
        assert!(out.contains("Closed"));
    }

    #[test]
    fn global_demand_multiplier() {
        let out = apply_scenario(BASE, 2.0, &[], None);
        assert!(out.contains("20") || out.contains("20.0"));
    }
}

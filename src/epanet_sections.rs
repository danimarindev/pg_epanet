//! EPANET-specific parsing on top of the generic INP section tokeniser (`mod inp`).

use std::collections::HashMap;

use crate::inp;

/// (pattern_id, point index, multiplier)
pub type PatternPoint = (String, i32, f64);

/// (curve_id, point index, x, y)
pub type CurvePoint = (String, i32, f64, f64);

/// (key, value)
pub type KeyValue = (String, String);

/// (rule_id, full rule block text)
pub type RuleBlock = (String, String);

pub fn parse_patterns(inp_text: &str) -> Vec<PatternPoint> {
    let sections = inp::parse_sections(inp_text);
    let rows = sections.get("PATTERNS").cloned().unwrap_or_default();
    let mut out = Vec::new();
    let mut idx_by_id: HashMap<String, i32> = HashMap::new();

    for fields in rows {
        if fields.is_empty() {
            continue;
        }
        let id = fields[0].clone();
        let start = idx_by_id.entry(id.clone()).or_insert(0);
        for (offset, field) in fields[1..].iter().enumerate() {
            if let Ok(mult) = field.parse::<f64>() {
                out.push((id.clone(), *start + offset as i32, mult));
            }
        }
        *start += (fields.len() - 1) as i32;
    }
    out
}

pub fn parse_curves(inp_text: &str) -> Vec<CurvePoint> {
    let sections = inp::parse_sections(inp_text);
    let rows = sections.get("CURVES").cloned().unwrap_or_default();
    let mut out = Vec::new();
    let mut idx_by_id: HashMap<String, i32> = HashMap::new();

    for fields in rows {
        if fields.len() < 3 {
            continue;
        }
        let id = fields[0].clone();
        let x: f64 = match fields[1].parse() {
            Ok(v) => v,
            Err(_) => continue,
        };
        let y: f64 = match fields[2].parse() {
            Ok(v) => v,
            Err(_) => continue,
        };
        let idx = idx_by_id.entry(id.clone()).or_insert(0);
        out.push((id, *idx, x, y));
        *idx += 1;
    }
    out
}

pub fn parse_key_value_section(inp_text: &str, section: &str) -> Vec<KeyValue> {
    let sections = inp::parse_sections(inp_text);
    let rows = sections.get(section).cloned().unwrap_or_default();
    rows.iter().filter_map(|fields| row_to_key_value(fields)).collect()
}

pub fn parse_controls(inp_text: &str) -> Vec<String> {
    let sections = inp::parse_sections(inp_text);
    sections
        .get("CONTROLS")
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .map(|fields| fields.join(" "))
        .filter(|line| !line.is_empty())
        .collect()
}

/// Parse [RULES] blocks terminated by a PRIORITY line.
pub fn parse_rules(inp_text: &str) -> Vec<RuleBlock> {
    let mut rules = Vec::new();
    let mut in_rules = false;
    let mut current_id: Option<String> = None;
    let mut current_lines: Vec<String> = Vec::new();

    for line in inp_text.lines() {
        let stripped = inp::strip_comment_line(line).trim();
        if stripped.is_empty() {
            continue;
        }
        if let Some(header) = inp::section_header(stripped) {
            if header == "RULES" {
                in_rules = true;
                continue;
            }
            if in_rules {
                flush_rule(&mut rules, &mut current_id, &mut current_lines);
                in_rules = false;
            }
            continue;
        }
        if !in_rules {
            continue;
        }

        let upper = stripped.to_ascii_uppercase();
        if upper.starts_with("RULE ") {
            flush_rule(&mut rules, &mut current_id, &mut current_lines);
            current_id = Some(stripped[5..].trim().to_string());
            current_lines.clear();
            continue;
        }

        current_lines.push(stripped.to_string());
        if upper.starts_with("PRIORITY ") {
            flush_rule(&mut rules, &mut current_id, &mut current_lines);
        }
    }
    if in_rules {
        flush_rule(&mut rules, &mut current_id, &mut current_lines);
    }
    rules
}

fn flush_rule(rules: &mut Vec<RuleBlock>, id: &mut Option<String>, lines: &mut Vec<String>) {
    if id.is_none() || lines.is_empty() {
        id.take();
        lines.clear();
        return;
    }
    let rule_id = id.take().unwrap();
    let text = lines.join("\n");
    lines.clear();
    if !text.is_empty() {
        rules.push((rule_id, text));
    }
}

pub fn parse_demands(inp_text: &str) -> Vec<(String, f64, Option<String>)> {
    let sections = inp::parse_sections(inp_text);
    let rows = sections.get("DEMANDS").cloned().unwrap_or_default();
    rows.into_iter()
        .filter_map(|fields| {
            if fields.len() < 2 {
                return None;
            }
            let junction = fields[0].clone();
            let demand: f64 = fields[1].parse().ok()?;
            let pattern = fields.get(2).cloned();
            Some((junction, demand, pattern))
        })
        .collect()
}

pub fn parse_emitters(inp_text: &str) -> Vec<(String, f64)> {
    let sections = inp::parse_sections(inp_text);
    let rows = sections.get("EMITTERS").cloned().unwrap_or_default();
    rows.into_iter()
        .filter_map(|fields| {
            if fields.len() < 2 {
                return None;
            }
            Some((fields[0].clone(), fields[1].parse().ok()?))
        })
        .collect()
}

pub fn parse_status(inp_text: &str) -> Vec<(String, String)> {
    let sections = inp::parse_sections(inp_text);
    let rows = sections.get("STATUS").cloned().unwrap_or_default();
    rows.into_iter()
        .filter_map(|fields| {
            if fields.len() < 2 {
                return None;
            }
            Some((fields[0].clone(), fields[1..].join(" ")))
        })
        .collect()
}

pub fn parse_sources(inp_text: &str) -> Vec<(String, String, f64, Option<String>)> {
    let sections = inp::parse_sections(inp_text);
    let rows = sections.get("SOURCES").cloned().unwrap_or_default();
    rows.into_iter()
        .filter_map(|fields| {
            if fields.len() < 3 {
                return None;
            }
            let node = fields[0].clone();
            let source_type = fields[1].clone();
            let quality: f64 = fields[2].parse().ok()?;
            let pattern = fields.get(3).cloned();
            Some((node, source_type, quality, pattern))
        })
        .collect()
}

fn row_to_key_value(fields: &[String]) -> Option<KeyValue> {
    if fields.is_empty() {
        return None;
    }
    if fields.len() == 1 {
        return Some((fields[0].clone(), String::new()));
    }
    if fields.len() >= 3 && is_likely_value_start(&fields[2]) {
        let key = format!("{} {}", fields[0], fields[1]);
        return Some((key, fields[2..].join(" ")));
    }
    Some((fields[0].clone(), fields[1..].join(" ")))
}

fn is_likely_value_start(s: &str) -> bool {
    if s.parse::<f64>().is_ok() {
        return true;
    }
    if s.contains(':') {
        return true;
    }
    matches!(
        s.to_ascii_uppercase().as_str(),
        "CONTINUE" | "STOP" | "NONE" | "YES" | "NO" | "OPEN" | "CLOSED"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"
[PATTERNS]
PD1 1.0 1.2
PD1 1.4 1.0

[CURVES]
HC1 0.0 40.0
HC1 10.0 35.0

[OPTIONS]
Units LPS
Demand Multiplier 1.5

[RULES]
RULE R1
IF TANK T1 LEVEL > 8
THEN PUMP PU1 STATUS = CLOSED
PRIORITY 1
"#;

    #[test]
    fn patterns_accumulate_same_id() {
        let pts = parse_patterns(SAMPLE);
        assert_eq!(pts.len(), 4);
        assert_eq!(pts[0], ("PD1".into(), 0, 1.0));
        assert_eq!(pts[3], ("PD1".into(), 3, 1.0));
    }

    #[test]
    fn curves_indexed_per_id() {
        let pts = parse_curves(SAMPLE);
        assert_eq!(pts.len(), 2);
        assert_eq!(pts[0].1, 0);
        assert_eq!(pts[1].1, 1);
    }

    #[test]
    fn options_two_word_key() {
        let kv = parse_key_value_section(SAMPLE, "OPTIONS");
        assert!(kv.iter().any(|(k, v)| k == "Demand Multiplier" && v == "1.5"));
    }

    #[test]
    fn rules_block_parsed() {
        let rules = parse_rules(SAMPLE);
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].0, "R1");
        assert!(rules[0].1.contains("PRIORITY 1"));
    }
}

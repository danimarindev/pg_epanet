//! Network topology and reference validation.

use pgrx::prelude::*;
use pgrx::spi::SpiResult;
use std::collections::{HashMap, HashSet, VecDeque};

#[derive(Clone, Debug)]
pub struct ValidationIssue {
    pub severity: String,
    pub issue_type: String,
    pub object_type: String,
    pub object_id: String,
    pub message: String,
}

pub fn validate_network(network_id: i32) -> Vec<ValidationIssue> {
    if !network_exists(network_id) {
        error!("No network found with id={network_id}");
    }

    let mut issues = Vec::new();
    let nodes = collect_nodes(network_id);
    let node_set: HashSet<String> = nodes.iter().cloned().collect();
    let patterns = collect_pattern_ids(network_id);
    let curves = collect_curve_ids(network_id);

    check_link_endpoints(network_id, &node_set, &mut issues);
    check_missing_coordinates(network_id, &node_set, &mut issues);
    check_pattern_references(network_id, &patterns, &mut issues);
    check_curve_references(network_id, &curves, &mut issues);
    check_orphan_junctions(network_id, &mut issues);
    check_disconnected_components(network_id, &nodes, &mut issues);

    issues
}

fn network_exists(network_id: i32) -> bool {
    Spi::get_one::<bool>(&format!(
        "SELECT EXISTS(SELECT 1 FROM epanet.networks WHERE id = {network_id})"
    ))
    .ok()
    .flatten()
    .unwrap_or(false)
}

fn collect_nodes(network_id: i32) -> Vec<String> {
    let mut nodes = Vec::new();
    for table in ["junctions", "tanks", "reservoirs"] {
        let _ = Spi::connect(|client| -> SpiResult<_> {
            let q = client.select(
                &format!("SELECT name FROM epanet.{table} WHERE network_id = {network_id}"),
                None,
                None,
            )?;
            for row in q {
                nodes.push(row.get_by_name::<String>("name")?.unwrap());
            }
            Ok(())
        });
    }
    nodes.sort();
    nodes.dedup();
    nodes
}

fn collect_pattern_ids(network_id: i32) -> HashSet<String> {
    let mut ids = HashSet::new();
    let _ = Spi::connect(|client| -> SpiResult<_> {
        let q = client.select(
            &format!(
                "SELECT DISTINCT pattern_id FROM epanet.patterns \
                 WHERE network_id = {network_id}"
            ),
            None,
            None,
        )?;
        for row in q {
            ids.insert(row.get_by_name::<String>("pattern_id")?.unwrap());
        }
        Ok(())
    });
    ids
}

fn collect_curve_ids(network_id: i32) -> HashSet<String> {
    let mut ids = HashSet::new();
    let _ = Spi::connect(|client| -> SpiResult<_> {
        let q = client.select(
            &format!(
                "SELECT DISTINCT curve_id FROM epanet.curves \
                 WHERE network_id = {network_id}"
            ),
            None,
            None,
        )?;
        for row in q {
            ids.insert(row.get_by_name::<String>("curve_id")?.unwrap());
        }
        Ok(())
    });
    ids
}

fn check_link_endpoints(network_id: i32, nodes: &HashSet<String>, issues: &mut Vec<ValidationIssue>) {
    for (table, link_col) in [
        ("pipes", "pipe"),
        ("pumps", "pump"),
        ("valves", "valve"),
    ] {
        let _ = Spi::connect(|client| -> SpiResult<_> {
            let q = client.select(
                &format!(
                    "SELECT name, node1, node2 FROM epanet.{table} \
                     WHERE network_id = {network_id}"
                ),
                None,
                None,
            )?;
            for row in q {
                let name: String = row.get_by_name("name")?.unwrap();
                let node1: String = row.get_by_name("node1")?.unwrap();
                let node2: String = row.get_by_name("node2")?.unwrap();
                for (node, end) in [(&node1, "node1"), (&node2, "node2")] {
                    if !nodes.contains(node) {
                        issues.push(ValidationIssue {
                            severity: "error".into(),
                            issue_type: "missing_node".into(),
                            object_type: link_col.into(),
                            object_id: name.clone(),
                            message: format!("{end} references unknown node '{node}'"),
                        });
                    }
                }
            }
            Ok(())
        });
    }
}

fn check_missing_coordinates(
    network_id: i32,
    nodes: &HashSet<String>,
    issues: &mut Vec<ValidationIssue>,
) {
    let mut with_coords = HashSet::new();
    let _ = Spi::connect(|client| -> SpiResult<_> {
        let q = client.select(
            &format!(
                "SELECT node_id FROM epanet.coordinates WHERE network_id = {network_id}"
            ),
            None,
            None,
        )?;
        for row in q {
            with_coords.insert(row.get_by_name::<String>("node_id")?.unwrap());
        }
        Ok(())
    });

    for node in nodes {
        if !with_coords.contains(node) {
            issues.push(ValidationIssue {
                severity: "warning".into(),
                issue_type: "missing_coordinate".into(),
                object_type: "node".into(),
                object_id: node.clone(),
                message: format!("Node '{node}' has no [COORDINATES] entry"),
            });
        }
    }
}

fn check_pattern_references(
    network_id: i32,
    patterns: &HashSet<String>,
    issues: &mut Vec<ValidationIssue>,
) {
    let check = |table: &str, id_col: &str, obj_type: &str| {
        let _ = Spi::connect(|client| -> SpiResult<_> {
            let q = client.select(
                &format!(
                    "SELECT {id_col} AS oid, pattern FROM epanet.{table} \
                     WHERE network_id = {network_id} AND pattern IS NOT NULL"
                ),
                None,
                None,
            )?;
            for row in q {
                let oid: String = row.get_by_name("oid")?.unwrap();
                let pattern: String = row.get_by_name("pattern")?.unwrap();
                if !patterns.contains(&pattern) {
                    issues.push(ValidationIssue {
                        severity: "error".into(),
                        issue_type: "dangling_pattern".into(),
                        object_type: obj_type.into(),
                        object_id: oid,
                        message: format!("References unknown pattern '{pattern}'"),
                    });
                }
            }
            Ok(())
        });
    };
    check("junctions", "name", "junction");
    check("demands", "junction_id", "demand");
    check("sources", "node_id", "source");
    check("pumps", "name", "pump");
}

fn check_curve_references(
    network_id: i32,
    curves: &HashSet<String>,
    issues: &mut Vec<ValidationIssue>,
) {
    let _ = Spi::connect(|client| -> SpiResult<_> {
        let q = client.select(
            &format!(
                "SELECT name, head_curve FROM epanet.pumps \
                 WHERE network_id = {network_id} AND head_curve IS NOT NULL"
            ),
            None,
            None,
        )?;
        for row in q {
            let name: String = row.get_by_name("name")?.unwrap();
            let curve: String = row.get_by_name("head_curve")?.unwrap();
            if !curves.contains(&curve) {
                issues.push(ValidationIssue {
                    severity: "error".into(),
                    issue_type: "dangling_curve".into(),
                    object_type: "pump".into(),
                    object_id: name,
                    message: format!("References unknown curve '{curve}'"),
                });
            }
        }
        Ok(())
    });

    let _ = Spi::connect(|client| -> SpiResult<_> {
        let q = client.select(
            &format!(
                "SELECT name, volume_curve FROM epanet.tanks \
                 WHERE network_id = {network_id} AND volume_curve IS NOT NULL"
            ),
            None,
            None,
        )?;
        for row in q {
            let name: String = row.get_by_name("name")?.unwrap();
            let curve: String = row.get_by_name("volume_curve")?.unwrap();
            if !curves.contains(&curve) {
                issues.push(ValidationIssue {
                    severity: "error".into(),
                    issue_type: "dangling_curve".into(),
                    object_type: "tank".into(),
                    object_id: name,
                    message: format!("References unknown curve '{curve}'"),
                });
            }
        }
        Ok(())
    });
}

fn check_orphan_junctions(network_id: i32, issues: &mut Vec<ValidationIssue>) {
    let _ = Spi::connect(|client| -> SpiResult<_> {
        let q = client.select(
            &format!(
                "SELECT j.name FROM epanet.junctions j \
                 WHERE j.network_id = {network_id} \
                 AND NOT EXISTS ( \
                     SELECT 1 FROM epanet.pipes p \
                     WHERE p.network_id = j.network_id \
                       AND (p.node1 = j.name OR p.node2 = j.name) \
                 ) \
                 AND NOT EXISTS ( \
                     SELECT 1 FROM epanet.pumps pu \
                     WHERE pu.network_id = j.network_id \
                       AND (pu.node1 = j.name OR pu.node2 = j.name) \
                 ) \
                 AND NOT EXISTS ( \
                     SELECT 1 FROM epanet.valves v \
                     WHERE v.network_id = j.network_id \
                       AND (v.node1 = j.name OR v.node2 = j.name) \
                 )"
            ),
            None,
            None,
        )?;
        for row in q {
            let name: String = row.get_by_name("name")?.unwrap();
            issues.push(ValidationIssue {
                severity: "warning".into(),
                issue_type: "orphan_junction".into(),
                object_type: "junction".into(),
                object_id: name.clone(),
                message: format!("Junction '{name}' has no incident links"),
            });
        }
        Ok(())
    });
}

fn check_disconnected_components(
    network_id: i32,
    nodes: &[String],
    issues: &mut Vec<ValidationIssue>,
) {
    if nodes.is_empty() {
        return;
    }

    let mut adj: HashMap<String, Vec<String>> = HashMap::new();
    for n in nodes {
        adj.entry(n.clone()).or_default();
    }

    let add_edge = |adj: &mut HashMap<String, Vec<String>>, a: &str, b: &str| {
        if adj.contains_key(a) && adj.contains_key(b) {
            adj.get_mut(a).unwrap().push(b.to_string());
            adj.get_mut(b).unwrap().push(a.to_string());
        }
    };

    for table in ["pipes", "pumps", "valves"] {
        let _ = Spi::connect(|client| -> SpiResult<_> {
            let q = client.select(
                &format!(
                    "SELECT node1, node2 FROM epanet.{table} WHERE network_id = {network_id}"
                ),
                None,
                None,
            )?;
            for row in q {
                let n1: String = row.get_by_name("node1")?.unwrap();
                let n2: String = row.get_by_name("node2")?.unwrap();
                add_edge(&mut adj, &n1, &n2);
            }
            Ok(())
        });
    }

    let mut visited: HashSet<String> = HashSet::new();
    let mut components = 0usize;
    for start in nodes {
        if visited.contains(start) {
            continue;
        }
        components += 1;
        let mut queue = VecDeque::new();
        queue.push_back(start.clone());
        visited.insert(start.clone());
        while let Some(n) = queue.pop_front() {
            if let Some(neighbors) = adj.get(&n) {
                for nb in neighbors {
                    if visited.insert(nb.clone()) {
                        queue.push_back(nb.clone());
                    }
                }
            }
        }
    }

    if components > 1 {
        issues.push(ValidationIssue {
            severity: "warning".into(),
            issue_type: "disconnected".into(),
            object_type: "network".into(),
            object_id: network_id.to_string(),
            message: format!("Network has {components} disconnected hydraulic components"),
        });
    }
}

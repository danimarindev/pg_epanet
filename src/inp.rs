use std::collections::HashMap;

/// Parsea el contenido de un fichero INP y devuelve un mapa de sección → líneas de campos.
/// Cada línea es un Vec<String> con los campos separados por whitespace, sin comentarios.
pub fn parse_sections(input: &str) -> HashMap<String, Vec<Vec<String>>> {
    let mut sections: HashMap<String, Vec<Vec<String>>> = HashMap::new();
    let mut current = String::new();

    for line in input.lines() {
        let line = strip_comment(line).trim();
        if line.is_empty() {
            continue;
        }
        if let Some(header) = section_header(line) {
            current = header;
            sections.entry(current.clone()).or_default();
        } else if !current.is_empty() {
            let fields: Vec<String> = line.split_whitespace().map(str::to_owned).collect();
            if !fields.is_empty() {
                sections.entry(current.clone()).or_default().push(fields);
            }
        }
    }
    sections
}

fn strip_comment(line: &str) -> &str {
    match line.find(';') {
        Some(pos) => &line[..pos],
        None => line,
    }
}

fn section_header(line: &str) -> Option<String> {
    if line.starts_with('[') {
        let end = line.find(']')?;
        Some(line[1..end].to_uppercase())
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_reservoirs() {
        let inp = "[RESERVOIRS]\n;ID   Head    Pattern\nR1   100.0\nR2   200.0   pat1\n";
        let sections = parse_sections(inp);
        let rows = sections.get("RESERVOIRS").unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0], vec!["R1", "100.0"]);
        assert_eq!(rows[1], vec!["R2", "200.0", "pat1"]);
    }

    #[test]
    fn test_parse_tanks_6_campos() {
        let inp = "[TANKS]\nT1  50.0  10.0  2.0  20.0  15.0\n";
        let sections = parse_sections(inp);
        let rows = sections.get("TANKS").unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].len(), 6);
    }

    #[test]
    fn test_parse_tanks_con_curva() {
        let inp = "[TANKS]\nT2  60.0  5.0  1.0  18.0  10.0  100.0  C1\n";
        let sections = parse_sections(inp);
        let rows = sections.get("TANKS").unwrap();
        assert_eq!(rows[0][7], "C1");
    }

    #[test]
    fn test_strip_comentario_inline() {
        let inp = "[TANKS]\nT1  50.0  10.0  2.0  20.0  15.0  ; comentario\n";
        let sections = parse_sections(inp);
        let rows = sections.get("TANKS").unwrap();
        assert_eq!(rows[0].len(), 6);
    }

    #[test]
    fn test_seccion_ausente() {
        let inp = "[JUNCTIONS]\nJ1  100.0\n";
        let sections = parse_sections(inp);
        assert!(sections.get("TANKS").is_none());
    }

    #[test]
    fn test_seccion_vacia() {
        let inp = "[TANKS]\n; solo comentarios\n\n";
        let sections = parse_sections(inp);
        let rows = sections.get("TANKS").unwrap();
        assert_eq!(rows.len(), 0);
    }

    #[test]
    fn test_cabecera_case_insensitive() {
        let inp = "[tanks]\nT1  50.0  10.0  2.0  20.0  15.0\n";
        let sections = parse_sections(inp);
        assert!(sections.contains_key("TANKS"));
    }
}

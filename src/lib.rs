use pgrx::prelude::*;

mod inp;

::pgrx::pg_module_magic!(name, version);

#[pg_extern]
fn hello_pg_epanet() -> &'static str {
    "Hello, pg_epanet"
}

/// Devuelve las filas de la sección [RESERVOIRS] del fichero INP.
#[pg_extern]
fn epanet_reservoirs(
    inp_text: &str,
) -> TableIterator<'static, (name!(name, String), name!(head, f64), name!(pattern, Option<String>))>
{
    let mut sections = inp::parse_sections(inp_text);
    let rows = sections.remove("RESERVOIRS").unwrap_or_default();

    TableIterator::new(rows.into_iter().filter_map(|fields| {
        if fields.len() < 2 {
            return None;
        }
        let name = fields[0].clone();
        let head: f64 = fields[1].parse().ok()?;
        let pattern = fields.get(2).cloned();
        Some((name, head, pattern))
    }))
}

/// Devuelve las filas de la sección [TANKS] del fichero INP.
/// min_volume toma valor 0.0 si no está presente en la línea.
/// volume_curve es NULL si no hay curva o si el valor es '*'.
#[pg_extern]
fn epanet_tanks(
    inp_text: &str,
) -> TableIterator<
    'static,
    (
        name!(name, String),
        name!(elevation, f64),
        name!(init_level, f64),
        name!(min_level, f64),
        name!(max_level, f64),
        name!(diameter, f64),
        name!(min_volume, f64),
        name!(volume_curve, Option<String>),
        name!(overflow, Option<String>),
    ),
>
{
    let mut sections = inp::parse_sections(inp_text);
    let rows = sections.remove("TANKS").unwrap_or_default();

    TableIterator::new(rows.into_iter().filter_map(|fields| {
        if fields.len() < 6 {
            return None;
        }
        let name = fields[0].clone();
        let elevation: f64 = fields[1].parse().ok()?;
        let init_level: f64 = fields[2].parse().ok()?;
        let min_level: f64 = fields[3].parse().ok()?;
        let max_level: f64 = fields[4].parse().ok()?;
        let diameter: f64 = fields[5].parse().ok()?;
        let min_volume: f64 = if fields.len() >= 7 {
            fields[6].parse().ok()?
        } else {
            0.0
        };
        let volume_curve: Option<String> = fields.get(7).and_then(|s| {
            if s == "*" { None } else { Some(s.clone()) }
        });
        let overflow: Option<String> = fields.get(8).cloned();
        Some((name, elevation, init_level, min_level, max_level, diameter, min_volume, volume_curve, overflow))
    }))
}

fn sql_text(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}

/// Crea el schema `epanet` y todas sus tablas la primera vez; idempotente.
/// Requiere PostGIS instalado — llamar solo tras verificar su presencia.
fn create_epanet_schema() {
    for sql in [
        "CREATE SCHEMA IF NOT EXISTS epanet",
        "CREATE TABLE IF NOT EXISTS epanet.networks (
            id          SERIAL PRIMARY KEY,
            name        TEXT NOT NULL,
            imported_at TIMESTAMPTZ NOT NULL DEFAULT now(),
            srid        INT NOT NULL,
            inp_text    TEXT NOT NULL
        )",
        "ALTER TABLE epanet.networks ADD COLUMN IF NOT EXISTS inp_text TEXT NOT NULL DEFAULT ''",
        "CREATE INDEX IF NOT EXISTS epanet_networks_name ON epanet.networks(name)",
        "CREATE TABLE IF NOT EXISTS epanet.junctions (
            network_id  INT NOT NULL REFERENCES epanet.networks(id) ON DELETE CASCADE,
            name        TEXT NOT NULL,
            elevation   FLOAT8 NOT NULL,
            demand      FLOAT8 NOT NULL,
            pattern     TEXT,
            geom        geometry(Point),
            PRIMARY KEY (network_id, name)
        )",
        "CREATE TABLE IF NOT EXISTS epanet.reservoirs (
            network_id  INT NOT NULL REFERENCES epanet.networks(id) ON DELETE CASCADE,
            name        TEXT NOT NULL,
            head        FLOAT8 NOT NULL,
            pattern     TEXT,
            geom        geometry(Point),
            PRIMARY KEY (network_id, name)
        )",
        "CREATE TABLE IF NOT EXISTS epanet.tanks (
            network_id   INT NOT NULL REFERENCES epanet.networks(id) ON DELETE CASCADE,
            name         TEXT NOT NULL,
            elevation    FLOAT8 NOT NULL,
            init_level   FLOAT8 NOT NULL,
            min_level    FLOAT8 NOT NULL,
            max_level    FLOAT8 NOT NULL,
            diameter     FLOAT8 NOT NULL,
            min_volume   FLOAT8 NOT NULL,
            volume_curve TEXT,
            overflow     TEXT,
            geom         geometry(Point),
            PRIMARY KEY (network_id, name)
        )",
        "CREATE TABLE IF NOT EXISTS epanet.pipes (
            network_id  INT NOT NULL REFERENCES epanet.networks(id) ON DELETE CASCADE,
            name        TEXT NOT NULL,
            node1       TEXT NOT NULL,
            node2       TEXT NOT NULL,
            length      FLOAT8 NOT NULL,
            diameter    FLOAT8 NOT NULL,
            roughness   FLOAT8 NOT NULL,
            minor_loss  FLOAT8 NOT NULL,
            status      TEXT NOT NULL,
            geom        geometry(LineString),
            PRIMARY KEY (network_id, name)
        )",
        "CREATE TABLE IF NOT EXISTS epanet.pumps (
            network_id  INT NOT NULL REFERENCES epanet.networks(id) ON DELETE CASCADE,
            name        TEXT NOT NULL,
            node1       TEXT NOT NULL,
            node2       TEXT NOT NULL,
            pump_type   TEXT,
            head_curve  TEXT,
            power       FLOAT8,
            speed       FLOAT8,
            pattern     TEXT,
            geom        geometry(LineString),
            PRIMARY KEY (network_id, name)
        )",
        "CREATE TABLE IF NOT EXISTS epanet.valves (
            network_id  INT NOT NULL REFERENCES epanet.networks(id) ON DELETE CASCADE,
            name        TEXT NOT NULL,
            node1       TEXT NOT NULL,
            node2       TEXT NOT NULL,
            diameter    FLOAT8 NOT NULL,
            valve_type  TEXT NOT NULL,
            setting     TEXT NOT NULL,
            minor_loss  FLOAT8 NOT NULL,
            geom        geometry(LineString),
            PRIMARY KEY (network_id, name)
        )",
        "CREATE TABLE IF NOT EXISTS epanet.coordinates (
            network_id  INT NOT NULL REFERENCES epanet.networks(id) ON DELETE CASCADE,
            node_id     TEXT NOT NULL,
            x           FLOAT8 NOT NULL,
            y           FLOAT8 NOT NULL,
            PRIMARY KEY (network_id, node_id)
        )",
        // idx preserva el orden de los vértices tal como aparecen en el INP
        "CREATE TABLE IF NOT EXISTS epanet.vertices (
            network_id  INT NOT NULL REFERENCES epanet.networks(id) ON DELETE CASCADE,
            link_id     TEXT NOT NULL,
            idx         INT NOT NULL,
            x           FLOAT8 NOT NULL,
            y           FLOAT8 NOT NULL,
            PRIMARY KEY (network_id, link_id, idx)
        )",
        // Vista unificada de todos los nodos (útil como capa GIS de puntos)
        "CREATE OR REPLACE VIEW epanet.nodes AS
            SELECT network_id, name AS node_id, 'junction'::text AS node_type, elevation, geom
              FROM epanet.junctions
            UNION ALL
            SELECT network_id, name, 'tank',      elevation, geom FROM epanet.tanks
            UNION ALL
            SELECT network_id, name, 'reservoir', head,      geom FROM epanet.reservoirs",
        // Tablas de resultados de simulación hidráulica
        "CREATE TABLE IF NOT EXISTS epanet.simulation_runs (
            id         SERIAL PRIMARY KEY,
            network_id INT NOT NULL REFERENCES epanet.networks(id) ON DELETE CASCADE,
            ran_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
            n_steps    INT NOT NULL
        )",
        "CREATE TABLE IF NOT EXISTS epanet.node_results (
            run_id   INT NOT NULL REFERENCES epanet.simulation_runs(id) ON DELETE CASCADE,
            step     INT NOT NULL,
            node_id  TEXT NOT NULL,
            head     DOUBLE PRECISION,
            pressure DOUBLE PRECISION,
            demand   DOUBLE PRECISION,
            PRIMARY KEY (run_id, step, node_id)
        )",
        "CREATE INDEX IF NOT EXISTS node_results_run ON epanet.node_results(run_id)",
        "CREATE TABLE IF NOT EXISTS epanet.link_results (
            run_id    INT NOT NULL REFERENCES epanet.simulation_runs(id) ON DELETE CASCADE,
            step      INT NOT NULL,
            link_id   TEXT NOT NULL,
            flow      DOUBLE PRECISION,
            velocity  DOUBLE PRECISION,
            headloss  DOUBLE PRECISION,
            PRIMARY KEY (run_id, step, link_id)
        )",
        "CREATE INDEX IF NOT EXISTS link_results_run ON epanet.link_results(run_id)",
    ] {
        Spi::run(sql).unwrap();
    }
}

/// Importa un fichero INP al catálogo interno `epanet` y genera geometría PostGIS.
/// Si ya existe una red con el mismo `network_name`, los datos anteriores se borran.
/// Requiere `CREATE EXTENSION postgis` en la base de datos.
/// Devuelve el `network_id` asignado.
#[pg_extern]
fn epanet_import(
    network_name: &str,
    inp_text: &str,
    srid: default!(i32, "5367"),
) -> i32 {
    let has_postgis = Spi::get_one::<bool>(
        "SELECT EXISTS(SELECT 1 FROM pg_extension WHERE extname = 'postgis')",
    )
    .unwrap()
    .unwrap_or(false);
    if !has_postgis {
        error!("PostGIS es requerido para epanet_import. Ejecuta: CREATE EXTENSION postgis;");
    }

    create_epanet_schema();

    let lit = sql_text(inp_text);
    let network_id = Spi::get_one::<i32>(&format!(
        "INSERT INTO epanet.networks(name, srid, inp_text) VALUES ({}, {srid}, {lit}) RETURNING id",
        sql_text(network_name)
    ))
    .unwrap()
    .unwrap();

    let nid = network_id;

    // Insertar datos usando las funciones epanet_* ya existentes
    for (table, func, cols) in [
        ("junctions",   "epanet_junctions",   "name, elevation, demand, pattern"),
        ("reservoirs",  "epanet_reservoirs",  "name, head, pattern"),
        ("tanks",       "epanet_tanks",       "name, elevation, init_level, min_level, max_level, diameter, min_volume, volume_curve, overflow"),
        ("pipes",       "epanet_pipes",       "name, node1, node2, length, diameter, roughness, minor_loss, status"),
        ("pumps",       "epanet_pumps",       "name, node1, node2, pump_type, head_curve, power, speed, pattern"),
        ("valves",      "epanet_valves",      "name, node1, node2, diameter, valve_type, setting, minor_loss"),
        ("coordinates", "epanet_coordinates", "node_id, x, y"),
    ] {
        Spi::run(&format!(
            "INSERT INTO epanet.{table}(network_id, {cols}) SELECT {nid}, * FROM {func}({lit})"
        ))
        .unwrap();
    }

    // Vértices con índice de orden para garantizar el orden correcto del LineString
    let mut sections = inp::parse_sections(inp_text);
    let vertex_rows = sections.remove("VERTICES").unwrap_or_default();
    if !vertex_rows.is_empty() {
        let mut link_idx: std::collections::HashMap<String, i32> = std::collections::HashMap::new();
        let values: Vec<String> = vertex_rows
            .iter()
            .filter_map(|f| {
                if f.len() != 3 { return None; }
                let x: f64 = f[1].parse().ok()?;
                let y: f64 = f[2].parse().ok()?;
                let idx = link_idx.entry(f[0].clone()).or_insert(0);
                let row = format!("({nid},{},{idx},{x},{y})", sql_text(&f[0]));
                *idx += 1;
                Some(row)
            })
            .collect();
        if !values.is_empty() {
            Spi::run(&format!(
                "INSERT INTO epanet.vertices(network_id, link_id, idx, x, y) VALUES {}",
                values.join(",")
            ))
            .unwrap();
        }
    }

    // Geometría de puntos para nodos (junctions, tanks, reservoirs)
    for table in ["junctions", "tanks", "reservoirs"] {
        Spi::run(&format!(
            "UPDATE epanet.{table} t \
             SET geom = ST_SetSRID(ST_MakePoint(c.x, c.y), {srid}) \
             FROM epanet.coordinates c \
             WHERE t.network_id = c.network_id AND t.name = c.node_id \
               AND t.network_id = {nid}"
        ))
        .unwrap();
    }

    // Geometría de líneas para tuberías (node1 + vértices ordenados + node2)
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
           AND p.network_id = {nid}"
    ))
    .unwrap();

    // Geometría de líneas para válvulas y bombas (segmento directo node1→node2)
    for table in ["valves", "pumps"] {
        Spi::run(&format!(
            "UPDATE epanet.{table} lnk \
             SET geom = ST_SetSRID( \
                 ST_MakeLine(ST_MakePoint(c1.x, c1.y), ST_MakePoint(c2.x, c2.y)), \
                 {srid}) \
             FROM epanet.coordinates c1, epanet.coordinates c2 \
             WHERE lnk.network_id = c1.network_id AND lnk.node1 = c1.node_id \
               AND lnk.network_id = c2.network_id AND lnk.node2 = c2.node_id \
               AND lnk.network_id = {nid}"
        ))
        .unwrap();
    }

    network_id
}

/// Ejecuta la simulación hidráulica de una red importada y almacena los resultados.
/// Usa la EPANET 2.3 C toolkit oficial (OWA-EPANET). Devuelve el run_id generado.
#[pg_extern]
fn epanet_simulate(network_id: i32) -> i32 {
    #[allow(non_upper_case_globals, clippy::useless_conversion)]
    use epanet_sys::*;
    use std::ffi::{CStr, CString};

    // 1. Leer el INP almacenado
    let inp_text = Spi::get_one::<String>(&format!(
        "SELECT inp_text FROM epanet.networks WHERE id = {network_id}"
    ))
    .unwrap()
    .unwrap_or_else(|| error!("No existe ninguna red con id={network_id}"));

    // 2. Escribir a fichero temporal
    let inp_path = format!("/tmp/pg_epanet_{network_id}.inp");
    let rpt_path = format!("/tmp/pg_epanet_{network_id}.rpt");
    let out_path = format!("/tmp/pg_epanet_{network_id}.out");
    std::fs::write(&inp_path, &inp_text)
        .unwrap_or_else(|e| error!("No se pudo escribir fichero temporal: {e}"));

    let c_inp = CString::new(inp_path.as_str()).unwrap();
    let c_rpt = CString::new(rpt_path.as_str()).unwrap();
    let c_out = CString::new(out_path.as_str()).unwrap();

    // 3. Abrir proyecto EPANET
    let ph = unsafe {
        let mut ph: EN_Project = std::ptr::null_mut();
        let ec = EN_createproject(&mut ph);
        if ec != 0 {
            let _ = std::fs::remove_file(&inp_path);
            error!("EN_createproject failed (code {})", ec);
        }
        let ec = EN_open(ph, c_inp.as_ptr(), c_rpt.as_ptr(), c_out.as_ptr());
        // EC=200 significa advertencias de formato pero el proyecto sigue abierto y usable.
        // Cualquier otro código != 0 es un error fatal.
        if ec != 0 && ec != 200 {
            EN_deleteproject(ph);
            let _ = std::fs::remove_file(&inp_path);
            let _ = std::fs::remove_file(&rpt_path);
            let mut buf = vec![0i8; 256];
            EN_geterror(ec, buf.as_mut_ptr(), 255);
            let msg = CStr::from_ptr(buf.as_ptr()).to_string_lossy().into_owned();
            error!("EN_open failed (code {}): {}", ec, msg);
        }
        ph
    };

    // 4. Resolver hidráulica completa (EPS)
    let ec = unsafe { EN_solveH(ph) };
    if ec > 1 {
        // código 0 = ok, código 1 = warnings no fatales
        unsafe { EN_close(ph); EN_deleteproject(ph); }
        let _ = std::fs::remove_file(&inp_path);
        let _ = std::fs::remove_file(&rpt_path);
        let _ = std::fs::remove_file(&out_path);
        error!("EN_solveH failed (code {})", ec);
    }

    // 5. Leer conteos
    let (n_nodes, n_links) = unsafe {
        let mut nn: i32 = 0;
        let mut nl: i32 = 0;
        EN_getcount(ph, EN_CountType_EN_NODECOUNT as i32, &mut nn);
        EN_getcount(ph, EN_CountType_EN_LINKCOUNT as i32, &mut nl);
        (nn, nl)
    };

    // 6. Registrar simulación
    let run_id = Spi::get_one::<i32>(&format!(
        "INSERT INTO epanet.simulation_runs(network_id, n_steps) \
         VALUES ({network_id}, 1) RETURNING id"
    ))
    .unwrap_or_else(|e| error!("SPI error al insertar simulation_run: {e:?}"))
    .unwrap();

    // 7. Recopilar resultados de nodos
    let node_vals: String = {
        let mut buf = vec![0i8; 64];
        let mut parts = Vec::with_capacity(n_nodes as usize);
        for i in 1..=n_nodes {
            unsafe {
                EN_getnodeid(ph, i, buf.as_mut_ptr());
                let mut head: f64 = 0.0;
                let mut pressure: f64 = 0.0;
                let mut demand: f64 = 0.0;
                EN_getnodevalue(ph, i, EN_NodeProperty_EN_HEAD as i32, &mut head);
                EN_getnodevalue(ph, i, EN_NodeProperty_EN_PRESSURE as i32, &mut pressure);
                EN_getnodevalue(ph, i, EN_NodeProperty_EN_DEMAND as i32, &mut demand);
                let name = CStr::from_ptr(buf.as_ptr())
                    .to_string_lossy().replace('\'', "''");
                // Convertir NaN/Inf a NULL para SQL
                let hv = if head.is_finite() { format!("{head}") } else { "NULL".into() };
                let pv = if pressure.is_finite() { format!("{pressure}") } else { "NULL".into() };
                let dv = if demand.is_finite() { format!("{demand}") } else { "NULL".into() };
                parts.push(format!("({run_id},0,'{name}',{hv},{pv},{dv})"));
            }
        }
        parts.join(",")
    };
    if !node_vals.is_empty() {
        Spi::run(&format!(
            "INSERT INTO epanet.node_results(run_id,step,node_id,head,pressure,demand) \
             VALUES {node_vals}"
        ))
        .unwrap_or_else(|e| error!("SPI error al insertar node_results: {e:?}"));
    }

    // 8. Recopilar resultados de enlaces
    let link_vals: String = {
        let mut buf = vec![0i8; 64];
        let mut parts = Vec::with_capacity(n_links as usize);
        for i in 1..=n_links {
            unsafe {
                EN_getlinkid(ph, i, buf.as_mut_ptr());
                let mut flow: f64 = 0.0;
                let mut velocity: f64 = 0.0;
                let mut headloss: f64 = 0.0;
                EN_getlinkvalue(ph, i, EN_LinkProperty_EN_FLOW as i32, &mut flow);
                EN_getlinkvalue(ph, i, EN_LinkProperty_EN_VELOCITY as i32, &mut velocity);
                EN_getlinkvalue(ph, i, EN_LinkProperty_EN_HEADLOSS as i32, &mut headloss);
                let name = CStr::from_ptr(buf.as_ptr())
                    .to_string_lossy().replace('\'', "''");
                let fv = if flow.is_finite() { format!("{flow}") } else { "NULL".into() };
                let vv = if velocity.is_finite() { format!("{velocity}") } else { "NULL".into() };
                let lv = if headloss.is_finite() { format!("{headloss}") } else { "NULL".into() };
                parts.push(format!("({run_id},0,'{name}',{fv},{vv},{lv})"));
            }
        }
        parts.join(",")
    };
    if !link_vals.is_empty() {
        Spi::run(&format!(
            "INSERT INTO epanet.link_results(run_id,step,link_id,flow,velocity,headloss) \
             VALUES {link_vals}"
        ))
        .unwrap_or_else(|e| error!("SPI error al insertar link_results: {e:?}"));
    }

    unsafe { EN_close(ph); EN_deleteproject(ph); }
    let _ = std::fs::remove_file(&inp_path);
    let _ = std::fs::remove_file(&rpt_path);
    let _ = std::fs::remove_file(&out_path);

    run_id
}

/// Devuelve las filas de la sección [JUNCTIONS] del fichero INP.
/// demand toma 0.0 si no está presente; pattern es NULL si no aparece.
#[pg_extern]
fn epanet_junctions(
    inp_text: &str,
) -> TableIterator<
    'static,
    (
        name!(name, String),
        name!(elevation, f64),
        name!(demand, f64),
        name!(pattern, Option<String>),
    ),
>
{
    let mut sections = inp::parse_sections(inp_text);
    let rows = sections.remove("JUNCTIONS").unwrap_or_default();

    TableIterator::new(rows.into_iter().filter_map(|fields| {
        if fields.len() < 2 {
            return None;
        }
        let name = fields[0].clone();
        let elevation: f64 = fields[1].parse().ok()?;
        let demand: f64 = fields.get(2).and_then(|s| s.parse().ok()).unwrap_or(0.0);
        let pattern = fields.get(3).cloned();
        Some((name, elevation, demand, pattern))
    }))
}

/// Devuelve las filas de la sección [PIPES] del fichero INP.
/// minor_loss toma 0.0 si no está presente; status toma 'OPEN' si no está presente.
/// 'CV' en status indica válvula de retención (check valve).
#[pg_extern]
fn epanet_pipes(
    inp_text: &str,
) -> TableIterator<
    'static,
    (
        name!(name, String),
        name!(node1, String),
        name!(node2, String),
        name!(length, f64),
        name!(diameter, f64),
        name!(roughness, f64),
        name!(minor_loss, f64),
        name!(status, String),
    ),
>
{
    let mut sections = inp::parse_sections(inp_text);
    let rows = sections.remove("PIPES").unwrap_or_default();

    TableIterator::new(rows.into_iter().filter_map(|fields| {
        if fields.len() < 6 {
            return None;
        }
        let name = fields[0].clone();
        let node1 = fields[1].clone();
        let node2 = fields[2].clone();
        let length: f64 = fields[3].parse().ok()?;
        let diameter: f64 = fields[4].parse().ok()?;
        let roughness: f64 = fields[5].parse().ok()?;
        let minor_loss: f64 = fields.get(6).and_then(|s| s.parse().ok()).unwrap_or(0.0);
        let status = fields.get(7).map(|s| s.to_uppercase()).unwrap_or_else(|| "OPEN".to_string());
        Some((name, node1, node2, length, diameter, roughness, minor_loss, status))
    }))
}

/// Devuelve las coordenadas X, Y de los nodos de la sección [COORDINATES].
#[pg_extern]
fn epanet_coordinates(
    inp_text: &str,
) -> TableIterator<'static, (name!(node_id, String), name!(x, f64), name!(y, f64))>
{
    let mut sections = inp::parse_sections(inp_text);
    let rows = sections.remove("COORDINATES").unwrap_or_default();

    TableIterator::new(rows.into_iter().filter_map(|fields| {
        if fields.len() < 3 {
            return None;
        }
        let node_id = fields[0].clone();
        let x: f64 = fields[1].parse().ok()?;
        let y: f64 = fields[2].parse().ok()?;
        Some((node_id, x, y))
    }))
}

/// Devuelve los vértices intermedios de tuberías de la sección [VERTICES].
/// Puede haber múltiples filas por tubería.
#[pg_extern]
fn epanet_vertices(
    inp_text: &str,
) -> TableIterator<'static, (name!(link_id, String), name!(x, f64), name!(y, f64))>
{
    let mut sections = inp::parse_sections(inp_text);
    let rows = sections.remove("VERTICES").unwrap_or_default();

    TableIterator::new(rows.into_iter().filter_map(|fields| {
        if fields.len() != 3 {
            return None;
        }
        let link_id = fields[0].clone();
        let x: f64 = fields[1].parse().ok()?;
        let y: f64 = fields[2].parse().ok()?;
        Some((link_id, x, y))
    }))
}

/// Devuelve las filas de la sección [PUMPS] del fichero INP.
/// Los campos del [3] en adelante son pares keyword-valor en cualquier orden.
/// pump_type es 'HEAD' o 'POWER'; speed es NULL si no aparece (EPANET asume 1.0).
#[pg_extern]
fn epanet_pumps(
    inp_text: &str,
) -> TableIterator<
    'static,
    (
        name!(name, String),
        name!(node1, String),
        name!(node2, String),
        name!(pump_type, Option<String>),
        name!(head_curve, Option<String>),
        name!(power, Option<f64>),
        name!(speed, Option<f64>),
        name!(pattern, Option<String>),
    ),
>
{
    let mut sections = inp::parse_sections(inp_text);
    let rows = sections.remove("PUMPS").unwrap_or_default();

    TableIterator::new(rows.into_iter().filter_map(|fields| {
        if fields.len() < 3 {
            return None;
        }
        let name = fields[0].clone();
        let node1 = fields[1].clone();
        let node2 = fields[2].clone();

        let mut pump_type: Option<String> = None;
        let mut head_curve: Option<String> = None;
        let mut power: Option<f64> = None;
        let mut speed: Option<f64> = None;
        let mut pattern: Option<String> = None;

        let mut i = 3;
        while i + 1 < fields.len() {
            match fields[i].to_uppercase().as_str() {
                "HEAD" => {
                    pump_type = Some("HEAD".to_string());
                    head_curve = Some(fields[i + 1].clone());
                }
                "POWER" => {
                    pump_type = Some("POWER".to_string());
                    power = fields[i + 1].parse().ok();
                }
                "SPEED" => {
                    speed = fields[i + 1].parse().ok();
                }
                "PATTERN" => {
                    pattern = Some(fields[i + 1].clone());
                }
                _ => {}
            }
            i += 2;
        }

        Some((name, node1, node2, pump_type, head_curve, power, speed, pattern))
    }))
}

/// Devuelve las filas de la sección [VALVES] del fichero INP.
/// setting es TEXT porque GPV almacena el nombre de una curva en ese campo.
/// minor_loss toma valor 0.0 si no está presente.
#[pg_extern]
fn epanet_valves(
    inp_text: &str,
) -> TableIterator<
    'static,
    (
        name!(name, String),
        name!(node1, String),
        name!(node2, String),
        name!(diameter, f64),
        name!(valve_type, String),
        name!(setting, String),
        name!(minor_loss, f64),
    ),
>
{
    let mut sections = inp::parse_sections(inp_text);
    let rows = sections.remove("VALVES").unwrap_or_default();

    TableIterator::new(rows.into_iter().filter_map(|fields| {
        if fields.len() < 6 {
            return None;
        }
        let name = fields[0].clone();
        let node1 = fields[1].clone();
        let node2 = fields[2].clone();
        let diameter: f64 = fields[3].parse().ok()?;
        let valve_type = fields[4].to_uppercase();
        let setting = fields[5].clone();
        let minor_loss: f64 = fields.get(6).and_then(|s| s.parse().ok()).unwrap_or(0.0);
        Some((name, node1, node2, diameter, valve_type, setting, minor_loss))
    }))
}

#[cfg(any(test, feature = "pg_test"))]
#[pg_schema]
mod tests {
    use pgrx::prelude::*;

    #[pg_test]
    fn test_hello_pg_epanet() {
        assert_eq!("Hello, pg_epanet", crate::hello_pg_epanet());
    }

    fn postgis_disponible() -> bool {
        Spi::get_one::<bool>(
            "SELECT EXISTS(SELECT 1 FROM pg_extension WHERE extname = 'postgis')",
        )
        .unwrap_or(Some(false))
        .unwrap_or(false)
    }

    #[pg_test]
    fn test_epanet_import_retorna_network_id_y_crea_tablas() {
        if !postgis_disponible() { return; }

        let inp = "$inp$
[JUNCTIONS]
J1  100.0  10.0  PD1
J2  200.0
[RESERVOIRS]
R1  300.0
[PIPES]
P1  J1  J2  100.0  200.0  0.01
[COORDINATES]
J1  100.0  200.0
J2  150.0  250.0
R1  50.0   100.0
[VERTICES]
P1  120.0  220.0
$inp$";

        let nid = Spi::get_one::<i32>(
            &format!("SELECT epanet_import('test_red', {inp}, 32632)"),
        )
        .unwrap()
        .unwrap();
        assert!(nid > 0);

        // Las tablas del catálogo deben tener los datos
        let n_j = Spi::get_one::<i64>(
            &format!("SELECT count(*)::bigint FROM epanet.junctions WHERE network_id = {nid}"),
        ).unwrap().unwrap();
        assert_eq!(n_j, 2);

        let n_p = Spi::get_one::<i64>(
            &format!("SELECT count(*)::bigint FROM epanet.pipes WHERE network_id = {nid}"),
        ).unwrap().unwrap();
        assert_eq!(n_p, 1);

        // Los nodos deben tener geometría generada
        let geom_notnull = Spi::get_one::<i64>(&format!(
            "SELECT count(*)::bigint FROM epanet.junctions \
             WHERE network_id = {nid} AND geom IS NOT NULL"
        )).unwrap().unwrap();
        assert_eq!(geom_notnull, 2);

        // La tubería debe tener geometría LineString
        let pipe_geom = Spi::get_one::<i64>(&format!(
            "SELECT count(*)::bigint FROM epanet.pipes \
             WHERE network_id = {nid} AND geom IS NOT NULL"
        )).unwrap().unwrap();
        assert_eq!(pipe_geom, 1);

        // El INP original debe estar almacenado
        let stored_len = Spi::get_one::<i32>(&format!(
            "SELECT length(inp_text) FROM epanet.networks WHERE id = {nid}"
        )).unwrap().unwrap();
        assert!(stored_len > 0);
    }

    #[pg_test]
    fn test_epanet_import_acumula_versiones() {
        if !postgis_disponible() { return; }

        let inp = "'[JUNCTIONS]\nJ1  50.0\n[COORDINATES]\nJ1  10.0  20.0\n'";
        let nid1 = Spi::get_one::<i32>(
            &format!("SELECT epanet_import('test_acum', {inp})"),
        ).unwrap().unwrap();
        let nid2 = Spi::get_one::<i32>(
            &format!("SELECT epanet_import('test_acum', {inp})"),
        ).unwrap().unwrap();

        // Cada llamada crea un network_id diferente
        assert_ne!(nid1, nid2);
        // Ambas versiones coexisten
        let n = Spi::get_one::<i64>(
            "SELECT count(*)::bigint FROM epanet.networks WHERE name = 'test_acum'",
        ).unwrap().unwrap();
        assert_eq!(n, 2);
    }

    #[pg_test]
    fn test_epanet_junctions_demand_default() {
        let (demand, pattern) = Spi::get_two::<f64, String>(
            "SELECT demand, pattern FROM epanet_junctions($inp$
[JUNCTIONS]
J1  100.0
$inp$)",
        )
        .unwrap();
        assert_eq!(demand.unwrap(), 0.0);
        assert!(pattern.is_none());
    }

    #[pg_test]
    fn test_epanet_junctions_con_demand_y_patron() {
        let (demand, pattern) = Spi::get_two::<f64, String>(
            "SELECT demand, pattern FROM epanet_junctions($inp$
[JUNCTIONS]
J1  100.0  50.0  PD1
$inp$) WHERE name = 'J1'",
        )
        .unwrap();
        assert_eq!(demand.unwrap(), 50.0);
        assert_eq!(pattern.unwrap().as_str(), "PD1");
    }

    #[pg_test]
    fn test_epanet_pipes_campos_minimos() {
        let (minor_loss, status) = Spi::get_two::<f64, String>(
            "SELECT minor_loss, status FROM epanet_pipes($inp$
[PIPES]
P1  J1  J2  100.0  200.0  0.01
$inp$)",
        )
        .unwrap();
        assert_eq!(minor_loss.unwrap(), 0.0);
        assert_eq!(status.unwrap().as_str(), "OPEN");
    }

    #[pg_test]
    fn test_epanet_pipes_con_todos_los_campos() {
        let inp = "$inp$
[PIPES]
P1  J1  J2  500.0  300.0  0.05  1.5  CLOSED
$inp$";
        let (length, roughness) = Spi::get_two::<f64, f64>(
            &format!("SELECT length, roughness FROM epanet_pipes({inp}) WHERE name = 'P1'"),
        )
        .unwrap();
        assert_eq!(length.unwrap(), 500.0);
        assert_eq!(roughness.unwrap(), 0.05);

        let (minor_loss, status) = Spi::get_two::<f64, String>(
            &format!("SELECT minor_loss, status FROM epanet_pipes({inp}) WHERE name = 'P1'"),
        )
        .unwrap();
        assert_eq!(minor_loss.unwrap(), 1.5);
        assert_eq!(status.unwrap().as_str(), "CLOSED");
    }

    #[pg_test]
    fn test_epanet_pipes_cv() {
        let status = Spi::get_one::<String>(
            "SELECT status FROM epanet_pipes($inp$
[PIPES]
P1  J1  J2  100.0  200.0  0.01  0.0  CV
$inp$)",
        )
        .unwrap()
        .unwrap();
        assert_eq!(status, "CV");
    }

    #[pg_test]
    fn test_epanet_pipes_status_mayusculas() {
        let status = Spi::get_one::<String>(
            "SELECT status FROM epanet_pipes($inp$
[PIPES]
P1  J1  J2  100.0  200.0  0.01  0.0  closed
$inp$)",
        )
        .unwrap()
        .unwrap();
        assert_eq!(status, "CLOSED");
    }

    #[pg_test]
    fn test_epanet_coordinates() {
        let (x, y) = Spi::get_two::<f64, f64>(
            "SELECT x, y FROM epanet_coordinates($inp$
[COORDINATES]
J1  100.5  200.75
$inp$) WHERE node_id = 'J1'",
        )
        .unwrap();
        assert_eq!(x.unwrap(), 100.5);
        assert_eq!(y.unwrap(), 200.75);
    }

    #[pg_test]
    fn test_epanet_vertices_multiples_por_tuberia() {
        let n = Spi::get_one::<i64>(
            "SELECT count(*)::bigint FROM epanet_vertices($inp$
[VERTICES]
P1  10.0  20.0
P1  15.0  25.0
P2  50.0  60.0
$inp$) WHERE link_id = 'P1'",
        )
        .unwrap()
        .unwrap();
        assert_eq!(n, 2);
    }

    #[pg_test]
    fn test_epanet_reservoirs_count() {
        let n = Spi::get_one::<i64>(
            "SELECT count(*)::bigint FROM epanet_reservoirs($inp$
[RESERVOIRS]
;ID    Head    Pattern
R1     100.0
R2     200.0   pat1
$inp$)",
        )
        .unwrap()
        .unwrap();
        assert_eq!(n, 2);
    }

    #[pg_test]
    fn test_epanet_reservoirs_valores() {
        let head = Spi::get_one::<f64>(
            "SELECT head FROM epanet_reservoirs($inp$
[RESERVOIRS]
R1  150.5
$inp$) WHERE name = 'R1'",
        )
        .unwrap()
        .unwrap();
        assert_eq!(head, 150.5);
    }

    #[pg_test]
    fn test_epanet_reservoirs_patron_nulo() {
        let pattern = Spi::get_one::<String>(
            "SELECT pattern FROM epanet_reservoirs($inp$
[RESERVOIRS]
R1  100.0
$inp$)",
        )
        .unwrap();
        // pattern debe ser NULL cuando no hay patrón
        assert!(pattern.is_none());
    }

    #[pg_test]
    fn test_epanet_tanks_count() {
        let n = Spi::get_one::<i64>(
            "SELECT count(*)::bigint FROM epanet_tanks($inp$
[TANKS]
;ID   Elev  InitLvl MinLvl MaxLvl Diam
T1    50.0  10.0    2.0    20.0   15.0
T2    60.0  5.0     1.0    18.0   10.0   100.0   C1
$inp$)",
        )
        .unwrap()
        .unwrap();
        assert_eq!(n, 2);
    }

    #[pg_test]
    fn test_epanet_tanks_min_volume_default() {
        let min_vol = Spi::get_one::<f64>(
            "SELECT min_volume FROM epanet_tanks($inp$
[TANKS]
T1  50.0  10.0  2.0  20.0  15.0
$inp$)",
        )
        .unwrap()
        .unwrap();
        assert_eq!(min_vol, 0.0);
    }

    #[pg_test]
    fn test_epanet_tanks_volume_curve() {
        let curve = Spi::get_one::<String>(
            "SELECT volume_curve FROM epanet_tanks($inp$
[TANKS]
T1  50.0  10.0  2.0  20.0  15.0  100.0  MiCurva
$inp$) WHERE name = 'T1'",
        )
        .unwrap()
        .unwrap();
        assert_eq!(curve, "MiCurva");
    }

    #[pg_test]
    fn test_epanet_tanks_asterisco_es_null() {
        let curve = Spi::get_one::<String>(
            "SELECT volume_curve FROM epanet_tanks($inp$
[TANKS]
T1  50.0  10.0  2.0  20.0  15.0  0.0  *
$inp$)",
        )
        .unwrap();
        assert!(curve.is_none());
    }

    #[pg_test]
    fn test_epanet_seccion_ausente_devuelve_vacio() {
        let n = Spi::get_one::<i64>(
            "SELECT count(*)::bigint FROM epanet_tanks('[JUNCTIONS]
J1  10.0
')",
        )
        .unwrap()
        .unwrap();
        assert_eq!(n, 0);
    }

    #[pg_test]
    fn test_epanet_pumps_head() {
        let (pump_type, head_curve, power) = Spi::get_three::<String, String, f64>(
            "SELECT pump_type, head_curve, power FROM epanet_pumps($inp$
[PUMPS]
P1  J1  J2  HEAD  HC1
$inp$) WHERE name = 'P1'",
        )
        .unwrap();
        assert_eq!(pump_type.unwrap().as_str(), "HEAD");
        assert_eq!(head_curve.unwrap().as_str(), "HC1");
        assert!(power.is_none());
    }

    #[pg_test]
    fn test_epanet_pumps_power_con_speed_y_pattern() {
        let inp = "$inp$
[PUMPS]
P2  J3  J4  POWER  10.5  SPEED  1.5  PATTERN  pat1
$inp$";
        let (pump_type, power) = Spi::get_two::<String, f64>(
            &format!("SELECT pump_type, power FROM epanet_pumps({inp}) WHERE name = 'P2'"),
        )
        .unwrap();
        assert_eq!(pump_type.unwrap().as_str(), "POWER");
        assert_eq!(power.unwrap(), 10.5);

        let (speed, pattern) = Spi::get_two::<f64, String>(
            &format!("SELECT speed, pattern FROM epanet_pumps({inp}) WHERE name = 'P2'"),
        )
        .unwrap();
        assert_eq!(speed.unwrap(), 1.5);
        assert_eq!(pattern.unwrap().as_str(), "pat1");
    }

    #[pg_test]
    fn test_epanet_pumps_speed_nula_si_ausente() {
        let speed = Spi::get_one::<f64>(
            "SELECT speed FROM epanet_pumps($inp$
[PUMPS]
P1  J1  J2  HEAD  HC1
$inp$)",
        )
        .unwrap();
        assert!(speed.is_none());
    }

    #[pg_test]
    fn test_epanet_valves_prv() {
        let (valve_type, setting, minor_loss) = Spi::get_three::<String, String, f64>(
            "SELECT valve_type, setting, minor_loss FROM epanet_valves($inp$
[VALVES]
V1  J1  J2  12.0  PRV  100.0  0.5
$inp$) WHERE name = 'V1'",
        )
        .unwrap();
        assert_eq!(valve_type.unwrap().as_str(), "PRV");
        assert_eq!(setting.unwrap().as_str(), "100.0");
        assert_eq!(minor_loss.unwrap(), 0.5);
    }

    #[pg_test]
    fn test_epanet_valves_gpv_setting_es_nombre_curva() {
        let setting = Spi::get_one::<String>(
            "SELECT setting FROM epanet_valves($inp$
[VALVES]
V2  J3  J4  10.0  GPV  GC1
$inp$) WHERE name = 'V2'",
        )
        .unwrap()
        .unwrap();
        assert_eq!(setting, "GC1");
    }

    #[pg_test]
    fn test_epanet_valves_minor_loss_default() {
        let minor_loss = Spi::get_one::<f64>(
            "SELECT minor_loss FROM epanet_valves($inp$
[VALVES]
V3  J5  J6  8.0  TCV  0.3
$inp$)",
        )
        .unwrap()
        .unwrap();
        assert_eq!(minor_loss, 0.0);
    }

    #[pg_test]
    fn test_epanet_valves_tipo_en_mayusculas() {
        let valve_type = Spi::get_one::<String>(
            "SELECT valve_type FROM epanet_valves($inp$
[VALVES]
V1  J1  J2  10.0  fcv  50.0
$inp$)",
        )
        .unwrap()
        .unwrap();
        assert_eq!(valve_type, "FCV");
    }
}

#[cfg(feature = "pg_bench")]
#[pg_schema]
mod benches {
    use pgrx::prelude::*;
    use pgrx_bench::{Bencher, black_box};

    #[pg_bench]
    fn bench_hello_pg_epanet(b: &mut Bencher) {
        b.iter(|| {
            black_box(crate::hello_pg_epanet());
        });
    }
}

/// This module is required by `cargo pgrx test` invocations.
/// It must be visible at the root of your extension crate.
#[cfg(test)]
pub mod pg_test {
    pub fn setup(_options: Vec<&str>) {}

    #[must_use]
    pub fn postgresql_conf_options() -> Vec<&'static str> {
        vec![]
    }
}

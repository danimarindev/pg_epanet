use pgrx::prelude::*;

extension_sql!(
    r#"
CREATE SCHEMA epanet;

CREATE TABLE epanet.networks (
    id          SERIAL PRIMARY KEY,
    name        TEXT NOT NULL,
    imported_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    srid        INT NOT NULL,
    inp_text    TEXT NOT NULL
);

CREATE INDEX epanet_networks_name ON epanet.networks(name);

CREATE TABLE epanet.junctions (
    network_id  INT NOT NULL REFERENCES epanet.networks(id) ON DELETE CASCADE,
    name        TEXT NOT NULL,
    elevation   FLOAT8 NOT NULL,
    demand      FLOAT8 NOT NULL,
    pattern     TEXT,
    geom        geometry(Point),
    PRIMARY KEY (network_id, name)
);

CREATE TABLE epanet.reservoirs (
    network_id  INT NOT NULL REFERENCES epanet.networks(id) ON DELETE CASCADE,
    name        TEXT NOT NULL,
    head        FLOAT8 NOT NULL,
    pattern     TEXT,
    geom        geometry(Point),
    PRIMARY KEY (network_id, name)
);

CREATE TABLE epanet.tanks (
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
);

CREATE TABLE epanet.pipes (
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
);

CREATE TABLE epanet.pumps (
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
);

CREATE TABLE epanet.valves (
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
);

CREATE TABLE epanet.coordinates (
    network_id  INT NOT NULL REFERENCES epanet.networks(id) ON DELETE CASCADE,
    node_id     TEXT NOT NULL,
    x           FLOAT8 NOT NULL,
    y           FLOAT8 NOT NULL,
    PRIMARY KEY (network_id, node_id)
);

CREATE TABLE epanet.vertices (
    network_id  INT NOT NULL REFERENCES epanet.networks(id) ON DELETE CASCADE,
    link_id     TEXT NOT NULL,
    idx         INT NOT NULL,
    x           FLOAT8 NOT NULL,
    y           FLOAT8 NOT NULL,
    PRIMARY KEY (network_id, link_id, idx)
);

CREATE VIEW epanet.nodes AS
    SELECT network_id, name AS node_id, 'junction'::text AS node_type, elevation, geom
      FROM epanet.junctions
    UNION ALL
    SELECT network_id, name, 'tank',      elevation, geom FROM epanet.tanks
    UNION ALL
    SELECT network_id, name, 'reservoir', head,      geom FROM epanet.reservoirs;

CREATE TABLE epanet.simulation_runs (
    id         SERIAL PRIMARY KEY,
    network_id INT NOT NULL REFERENCES epanet.networks(id) ON DELETE CASCADE,
    ran_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    n_steps    INT NOT NULL
);

CREATE TABLE epanet.node_results (
    run_id   INT NOT NULL REFERENCES epanet.simulation_runs(id) ON DELETE CASCADE,
    step     INT NOT NULL,
    node_id  TEXT NOT NULL,
    head     DOUBLE PRECISION,
    pressure DOUBLE PRECISION,
    demand   DOUBLE PRECISION,
    PRIMARY KEY (run_id, step, node_id)
);

CREATE INDEX node_results_run ON epanet.node_results(run_id);

CREATE TABLE epanet.link_results (
    run_id    INT NOT NULL REFERENCES epanet.simulation_runs(id) ON DELETE CASCADE,
    step      INT NOT NULL,
    link_id   TEXT NOT NULL,
    flow      DOUBLE PRECISION,
    velocity  DOUBLE PRECISION,
    headloss  DOUBLE PRECISION,
    PRIMARY KEY (run_id, step, link_id)
);

CREATE INDEX link_results_run ON epanet.link_results(run_id);

CREATE INDEX junctions_geom  ON epanet.junctions  USING GIST (geom);
CREATE INDEX reservoirs_geom ON epanet.reservoirs USING GIST (geom);
CREATE INDEX tanks_geom      ON epanet.tanks      USING GIST (geom);
CREATE INDEX pipes_geom      ON epanet.pipes      USING GIST (geom);
CREATE INDEX pumps_geom      ON epanet.pumps      USING GIST (geom);
CREATE INDEX valves_geom     ON epanet.valves     USING GIST (geom);

CREATE TABLE epanet.patterns (
    network_id  INT NOT NULL REFERENCES epanet.networks(id) ON DELETE CASCADE,
    pattern_id  TEXT NOT NULL,
    idx         INT NOT NULL,
    multiplier  FLOAT8 NOT NULL,
    PRIMARY KEY (network_id, pattern_id, idx)
);

CREATE TABLE epanet.curves (
    network_id  INT NOT NULL REFERENCES epanet.networks(id) ON DELETE CASCADE,
    curve_id    TEXT NOT NULL,
    idx         INT NOT NULL,
    x           FLOAT8 NOT NULL,
    y           FLOAT8 NOT NULL,
    PRIMARY KEY (network_id, curve_id, idx)
);

CREATE TABLE epanet.options (
    network_id  INT NOT NULL REFERENCES epanet.networks(id) ON DELETE CASCADE,
    key         TEXT NOT NULL,
    value       TEXT NOT NULL,
    PRIMARY KEY (network_id, key)
);

CREATE TABLE epanet.times (
    network_id  INT NOT NULL REFERENCES epanet.networks(id) ON DELETE CASCADE,
    key         TEXT NOT NULL,
    value       TEXT NOT NULL,
    PRIMARY KEY (network_id, key)
);

CREATE TABLE epanet.controls (
    network_id  INT NOT NULL REFERENCES epanet.networks(id) ON DELETE CASCADE,
    idx         INT NOT NULL,
    rule_text   TEXT NOT NULL,
    PRIMARY KEY (network_id, idx)
);

CREATE TABLE epanet.rules (
    network_id  INT NOT NULL REFERENCES epanet.networks(id) ON DELETE CASCADE,
    rule_id     TEXT NOT NULL,
    rule_text   TEXT NOT NULL,
    PRIMARY KEY (network_id, rule_id)
);

CREATE TABLE epanet.demands (
    network_id  INT NOT NULL REFERENCES epanet.networks(id) ON DELETE CASCADE,
    junction_id TEXT NOT NULL,
    demand      FLOAT8 NOT NULL,
    pattern     TEXT,
    PRIMARY KEY (network_id, junction_id)
);

CREATE TABLE epanet.emitters (
    network_id  INT NOT NULL REFERENCES epanet.networks(id) ON DELETE CASCADE,
    junction_id TEXT NOT NULL,
    coefficient FLOAT8 NOT NULL,
    PRIMARY KEY (network_id, junction_id)
);

CREATE TABLE epanet.status (
    network_id  INT NOT NULL REFERENCES epanet.networks(id) ON DELETE CASCADE,
    link_id     TEXT NOT NULL,
    status_value TEXT NOT NULL,
    PRIMARY KEY (network_id, link_id)
);

CREATE TABLE epanet.sources (
    network_id   INT NOT NULL REFERENCES epanet.networks(id) ON DELETE CASCADE,
    node_id      TEXT NOT NULL,
    source_type  TEXT NOT NULL,
    quality      FLOAT8 NOT NULL,
    pattern      TEXT,
    PRIMARY KEY (network_id, node_id)
);

CREATE TABLE epanet.reactions (
    network_id  INT NOT NULL REFERENCES epanet.networks(id) ON DELETE CASCADE,
    key         TEXT NOT NULL,
    value       TEXT NOT NULL,
    PRIMARY KEY (network_id, key)
);

CREATE TABLE epanet.quality (
    network_id  INT NOT NULL REFERENCES epanet.networks(id) ON DELETE CASCADE,
    key         TEXT NOT NULL,
    value       TEXT NOT NULL,
    PRIMARY KEY (network_id, key)
);

CREATE TABLE epanet.energy (
    network_id  INT NOT NULL REFERENCES epanet.networks(id) ON DELETE CASCADE,
    key         TEXT NOT NULL,
    value       TEXT NOT NULL,
    PRIMARY KEY (network_id, key)
);

CREATE TABLE epanet.report (
    network_id  INT NOT NULL REFERENCES epanet.networks(id) ON DELETE CASCADE,
    key         TEXT NOT NULL,
    value       TEXT NOT NULL,
    PRIMARY KEY (network_id, key)
);
"#,
    name = "epanet_schema",
    bootstrap,
);

# pg_epanet

PostgreSQL extension (written in Rust via [pgrx](https://github.com/pgcentralfoundation/pgrx)) that parses EPANET `.inp` water network files and materialises them as queryable SQL tables with PostGIS geometry.

> **Status:** v0.3.0 in development on `main` — [v0.2.1](https://github.com/danimarindev/pg_epanet/releases/tag/v0.2.1) is the latest release. See [CHANGELOG.md](CHANGELOG.md).

## Why pg_epanet?

Tools like WNTR or swmm-api parse an INP file into Python objects in memory. `pg_epanet` parses the INP **directly into PostgreSQL tables**: queryable with SQL, joinable with any other data, with PostGIS geometry ready to use as a GIS layer.

For users who already store infrastructure data in PostGIS this eliminates the intermediate pipeline of "export INP → Python script → re-import results". For bulk scenarios (importing thousands of INP files in parallel inside the database, e.g. resilience studies) the Rust implementation also provides a meaningful speed advantage.

## Features

- **Import** an EPANET INP into permanent tables with a single function call
- **PostGIS geometry** — nodes as `Point`, pipes as `LineString` (with ordered `[VERTICES]`), pumps/valves as direct links
- **Hydraulic EPS simulation** via the official OWA-EPANET 2.3 C toolkit (statically linked — no external install)
- **Water quality EPS** — `epanet_simulate_quality` stores per-timestep node/link quality on an existing hydraulic run
- **Per-timestep results** stored in `node_results` and `link_results` (`step` column, full Extended Period Simulation)
- **Schema bootstrap** — all tables, indexes, and views created at `CREATE EXTENSION pg_epanet`
- **Delete** networks and all associated data with `epanet_delete`
- Works on **managed PostgreSQL** (RDS, Supabase, etc.) — the INP is passed as `text`, no server filesystem read required for import

### INP sections supported (v0.2.0)

**Topology & geometry:** `[JUNCTIONS]`, `[RESERVOIRS]`, `[TANKS]`, `[PIPES]`, `[PUMPS]`, `[VALVES]`, `[COORDINATES]`, `[VERTICES]`.

**Metadata:** `[PATTERNS]`, `[CURVES]`, `[OPTIONS]`, `[TIMES]`, `[CONTROLS]`, `[RULES]`, `[DEMANDS]`, `[EMITTERS]`, `[STATUS]`, `[SOURCES]`, `[REACTIONS]`, `[QUALITY]`, `[ENERGY]`, `[REPORT]`.

`epanet_simulate` still reads `networks.inp_text` verbatim — metadata tables are for SQL queryability and future export, not yet used to reconstruct INP for simulation.

## Requirements

- PostgreSQL 13–18 (pgrx features for each major version; development target is PG 18)
- [PostGIS](https://postgis.net/) — installed automatically with `CREATE EXTENSION pg_epanet CASCADE`
- Rust + [pgrx](https://github.com/pgcentralfoundation/pgrx) 0.19.1 (for building from source)
- For `epanet_simulate`: the Postgres server process must be able to write temporary files under `/tmp`

## Docker

```bash
docker compose up -d
psql "postgresql://postgres:pg_epanet@localhost:5432/pg_epanet" -c "SELECT extname FROM pg_extension;"
```

The image uses `postgres:18-trixie` with PostGIS 3 from PGDG (native arm64; the `postgis/postgis:18-3.6` tag is amd64-only). The extension is pre-installed on first database init.

## Installing a release

Check out a tagged version and build against your PostgreSQL installation:

```bash
git checkout v0.2.1
cargo install cargo-pgrx --version '=0.19.1'
cargo pgrx init
cargo pgrx install --release --features pg18 --no-default-features --pg-config $(which pg_config)
```

Then in psql:

```sql
CREATE EXTENSION pg_epanet CASCADE;
```

Or use Docker (see above) for a ready-made PostgreSQL 18 + PostGIS + pg_epanet stack.

### Upgrading

```sql
-- From 0.1.0
ALTER EXTENSION pg_epanet UPDATE TO '0.2.0';

-- Re-import networks to populate metadata tables (upgrade does not backfill)
SELECT epanet_import(name || '_v2', inp_text, srid)
FROM epanet.networks;
```

-- From 0.2.0 (indexes only)
ALTER EXTENSION pg_epanet UPDATE TO '0.2.1';

## Usage guide

Typical workflow: install extension → import INP → query topology/metadata → simulate → analyse results.

### 1. Install

```sql
CREATE EXTENSION pg_epanet CASCADE;   -- pulls in PostGIS
```

### 2. Import a network

Pass the full INP as `text`. Returns `network_id` (integer PK in `epanet.networks`).

```sql
SELECT epanet_import(
  'downtown',          -- name (not unique; each import creates a new row)
  pg_read_file('/path/on/server/network.inp'),  -- or $inp$ ... $inp$ from client
  25830                -- SRID for [COORDINATES] → PostGIS geom
) AS network_id;
```

From psql with a local file (client-side read):

```sql
\set content `cat tests/fixtures/simple.inp`
SELECT epanet_import('simple', :'content', 4326);
```

Every `[JUNCTIONS]`, `[PIPES]`, … section listed under [Features](#inp-sections-supported-v020) is parsed once at import and stored in `epanet.*` tables. The raw INP is also kept in `networks.inp_text` for simulation.

### 3. Explore topology (SQL + GIS)

```sql
-- All nodes with geometry (junctions + tanks + reservoirs)
SELECT node_id, node_type, elevation, ST_AsText(geom) AS wkt
FROM epanet.nodes
WHERE network_id = 1;

-- Pipes longer than 500 m
SELECT name, length, diameter, status
FROM epanet.pipes
WHERE network_id = 1 AND length > 500;

-- Spatial filter: nodes inside a polygon (use your AOI table)
SELECT j.name, j.demand
FROM epanet.junctions j
JOIN public.my_aoi a ON ST_Within(j.geom, a.geom)
WHERE j.network_id = 1;

-- Graph: links incident on a junction (uses pipes_node1 / pipes_node2 indexes)
SELECT p.name, p.node1, p.node2, p.diameter
FROM epanet.pipes p
WHERE p.network_id = 1 AND (p.node1 = 'J1' OR p.node2 = 'J1');
```

Join pipes to junctions for attribute + geometry overlays:

```sql
SELECT p.name AS pipe, j1.name AS from_node, j2.name AS to_node, p.geom
FROM epanet.pipes p
JOIN epanet.junctions j1 ON j1.network_id = p.network_id AND j1.name = p.node1
JOIN epanet.junctions j2 ON j2.network_id = p.network_id AND j2.name = p.node2
WHERE p.network_id = 1;
```

### 4. Query metadata

```sql
-- Demand pattern multipliers
SELECT pattern_id, idx, multiplier
FROM epanet.patterns
WHERE network_id = 1 AND pattern_id = 'PD1'
ORDER BY idx;

-- Pump head curve points
SELECT curve_id, idx, x, y
FROM epanet.curves
WHERE network_id = 1 AND curve_id = 'HC1'
ORDER BY idx;

-- Simulation options
SELECT key, value FROM epanet.options WHERE network_id = 1;

-- Rule-based controls (full text block)
SELECT rule_id, rule_text FROM epanet.rules WHERE network_id = 1;
```

Ad-hoc parse without import (re-parses INP on every query — fine for small files):

```sql
SELECT * FROM epanet_patterns((SELECT inp_text FROM epanet.networks WHERE id = 1));
```

### 5. Run hydraulic simulation (EPS)

```sql
SET client_min_messages TO warning;   -- see EPANET solver warnings (codes 1–99)

SELECT epanet_simulate(1) AS run_id;
-- → inserts one row in simulation_runs, bulk rows in node_results / link_results
```

Simulation reads `networks.inp_text` verbatim (patterns, curves, controls in the INP are used even if you only changed SQL tables).

```sql
-- Latest run for a network
SELECT id, ran_at, n_steps
FROM epanet.simulation_runs
WHERE network_id = 1
ORDER BY id DESC
LIMIT 1;

-- Pressure envelope across all timesteps
SELECT node_id,
       min(pressure) AS min_p,
       max(pressure) AS max_p
FROM epanet.node_results
WHERE run_id = 1
GROUP BY node_id
ORDER BY min_p;

-- One timestep, join to geometry
SELECT r.step, r.node_id, r.pressure, n.geom
FROM epanet.node_results r
JOIN epanet.nodes n ON n.network_id = 1 AND n.node_id = r.node_id
WHERE r.run_id = 1 AND r.step = 0;
```

### 6. Delete a network

```sql
SELECT epanet_delete(1);   -- CASCADE: topology, metadata, simulation_runs, results
```

### 7. Bulk import (many scenarios in parallel)

Each `epanet_import` call is independent — safe to run from multiple sessions:

```sql
-- Example: one INP per row in a staging table
CREATE TABLE scenarios (name text, inp text, srid int);
-- ... load rows ...

SELECT s.name, epanet_import(s.name, s.inp, s.srid) AS network_id
FROM scenarios s;
```

For very large INPs, use the `\COPY` + `string_agg` pattern below to avoid passing multi-MB strings on the wire twice.

## Performance & indexes

All entity tables use composite primary keys starting with `network_id`, so `WHERE network_id = $1` hits the PK B-tree prefix on every table.

| Index | Table | Purpose |
|-------|-------|---------|
| `epanet_networks_name` | `networks` | lookup by name |
| `junctions_geom` … `valves_geom` | topology | GiST spatial queries (`ST_Within`, `&&`, etc.) |
| `pipes_node1`, `pipes_node2` | `pipes` | incident links / graph traversal |
| `pumps_node1`, `pumps_node2` | `pumps` | same |
| `valves_node1`, `valves_node2` | `valves` | same |
| `simulation_runs_network` | `simulation_runs` | list runs per network |
| `node_results_run` | `node_results` | filter results by `run_id` (PK prefix also covers this) |
| `link_results_run` | `link_results` | same |

Metadata tables (`patterns`, `curves`, `options`, …) rely on `(network_id, …)` PKs — no extra indexes needed for typical per-network queries.

**Tips**

- Always filter by `network_id` first — every hot path assumes it.
- Prefer imported tables over table-returning functions for repeated queries (parse once at import).
- For EPS result tables with millions of rows, filter by `run_id` and optionally `step`; consider partitioning by `run_id` (backlog — see ROADMAP).
- `epanet_simulate` writes temp files under `/tmp` on the server; bulk parallel runs need disk headroom there.

## Quick start

```sql
-- 1. Enable pg_epanet (PostGIS is installed automatically via CASCADE)
CREATE EXTENSION pg_epanet CASCADE;

-- 2. Import a network (pass the INP file content as text)
SELECT epanet_import('my_network', $inp$
[TITLE]
...
[END]
$inp$, 4326) AS network_id;

-- 3. Query the imported data
SELECT name, elevation, demand, ST_AsText(geom) AS wkt
FROM epanet.junctions
WHERE network_id = 1;

-- 4. Run a hydraulic simulation (EPS — all timesteps)
SELECT epanet_simulate(1) AS run_id;

-- 5b. Water quality (requires Quality ≠ NONE in INP; run hydraulic first)
SELECT epanet_simulate_quality(1, 1);

-- 6. Inspect results (filter by timestep if needed)
SELECT step, node_id, round(pressure::numeric, 2) AS pressure_m
FROM epanet.node_results
WHERE run_id = 1
ORDER BY step, pressure ASC
LIMIT 10;
```

### Loading a large INP file from psql

```sql
-- Use a SERIAL column to preserve line order
CREATE TEMP TABLE _inp (lineno SERIAL, data text);
\COPY _inp(data) FROM '/path/to/network.inp'

WITH inp AS (SELECT string_agg(data, E'\n' ORDER BY lineno) AS txt FROM _inp)
SELECT epanet_import('my_network', txt, 25830) FROM inp;
```

> **Note:** Always use `ORDER BY lineno` (or equivalent sequence column), not `ORDER BY ctid`. For files with thousands of lines, `ctid` ordering is not guaranteed to match insertion order.

## SQL reference

### Import

```sql
epanet_import(network_name text, inp_text text, srid int DEFAULT 5367) → int
```

Parses the INP and writes all supported sections to permanent tables under the `epanet` schema. Returns the `network_id`. Each call creates a new network row; same-name networks are not replaced.

### Simulation

```sql
epanet_simulate(network_id int) → int
```

Runs a full Extended Period Simulation (EPS) using OWA-EPANET 2.3. Stores per-timestep results in `epanet.node_results` (head, pressure, demand) and `epanet.link_results` (flow, velocity, headloss). Returns the `run_id`. Fatal solver errors (code ≥ 100) abort the simulation; warning codes 1–99 are emitted as PostgreSQL `WARNING` messages (with timestep and EPANET error text).

### Water quality simulation

```sql
epanet_simulate_quality(network_id int, run_id int) → int
```

Runs water quality EPS for an existing hydraulic run (`run_id` from `epanet_simulate`). Re-runs hydraulics interleaved with quality routing, then stores results in `epanet.node_quality_results` and `epanet.link_quality_results`. Returns the same `run_id`. Requires `[OPTIONS] Quality` to be `CHEMICAL`, `AGE`, or `TRACE` (not `NONE`).

```sql
epanet_count_nodes_below_threshold(run_id int, threshold float8) → bigint
```

Counts nodes whose minimum quality across all timesteps falls below `threshold` (uses `epanet.node_quality_envelope`).

### Delete

```sql
epanet_delete(network_id int) → boolean
```

Deletes a network row and all associated topology and simulation results (CASCADE). Errors if the id does not exist.

### Table-returning functions (parse on the fly)

These functions parse the INP text in-query and return rows — useful for ad-hoc inspection without importing.

| Function | Returns |
|---|---|
| `epanet_junctions(inp_text)` | name, elevation, demand, pattern |
| `epanet_reservoirs(inp_text)` | name, head, pattern |
| `epanet_tanks(inp_text)` | name, elevation, levels, diameter, min_volume, volume_curve, overflow |
| `epanet_pipes(inp_text)` | name, node1, node2, length, diameter, roughness, minor_loss, status |
| `epanet_pumps(inp_text)` | name, node1, node2, pump_type, head_curve, power, speed, pattern |
| `epanet_valves(inp_text)` | name, node1, node2, diameter, valve_type, setting, minor_loss |
| `epanet_coordinates(inp_text)` | node_id, x, y |
| `epanet_vertices(inp_text)` | link_id, idx, x, y |
| `epanet_patterns(inp_text)` | pattern_id, idx, multiplier |
| `epanet_curves(inp_text)` | curve_id, idx, x, y |
| `epanet_options(inp_text)` | key, value |
| `epanet_times(inp_text)` | key, value |
| `epanet_controls(inp_text)` | idx, rule_text |
| `epanet_rules(inp_text)` | rule_id, rule_text |
| `epanet_demands(inp_text)` | junction_id, demand, pattern |
| `epanet_emitters(inp_text)` | junction_id, coefficient |
| `epanet_status(inp_text)` | link_id, status_value |
| `epanet_sources(inp_text)` | node_id, source_type, quality, pattern |
| `epanet_reactions(inp_text)` | key, value |
| `epanet_quality(inp_text)` | key, value |
| `epanet_energy(inp_text)` | key, value |
| `epanet_report(inp_text)` | key, value |

## Schema

Created at `CREATE EXTENSION pg_epanet`:

**Network catalogue**
- `networks` — one row per imported network (name, SRID, import timestamp, original INP text)

**Topology**
- `junctions`, `reservoirs`, `tanks` — nodes with `geom geometry(Point)` and GiST index
- `pipes`, `pumps`, `valves` — links with `geom geometry(LineString)`, GiST index, and `(network_id, node1/node2)` B-tree indexes
- `coordinates`, `vertices` — raw geometry data (vertices ordered by `idx`)
- `nodes` — unified view of all node types

**Metadata**
- `patterns`, `curves` — time multipliers and curve points (PK: `network_id`, id, `idx`)
- `options`, `times`, `reactions`, `quality`, `energy`, `report` — key-value settings
- `controls`, `rules` — control logic stored as full rule text
- `demands`, `emitters`, `status`, `sources` — per-element overrides

**Simulation results**
- `simulation_runs` — one row per simulation run (`n_steps` = timesteps solved); indexed on `network_id`
- `node_results` — head, pressure, demand per node per `step` (PK: `run_id`, `step`, `node_id`)
- `link_results` — flow, velocity, headloss per link per `step`
- `node_quality_results` — quality (concentration, age, or trace) per node per `step`
- `link_quality_results` — average link quality per `step`
- `node_quality_envelope` — view with min/max/avg quality per node per run

See [Performance & indexes](#performance--indexes) for the full index list and query tips.

## Building from source

```bash
cargo install cargo-pgrx --version '=0.19.1'
cargo pgrx init
cargo pgrx run pg18 --features pg18 --no-default-features   # compile, start sandbox, open psql
```

Inside psql after code changes:

```sql
DROP EXTENSION pg_epanet CASCADE;
CREATE EXTENSION pg_epanet CASCADE;
```

Run the test suite:

```bash
cargo pgrx test pg18 --features pg18 --no-default-features
```

## Releasing

Releases are managed with the Rust `xtask` tool (Keep a Changelog + GitHub Releases):

```bash
# write changes under ## [Unreleased] in CHANGELOG.md, then:
cargo xtask release 0.2.0 --create-github-release --yes

# if the tag is already pushed but GitHub Release failed:
cargo xtask github-release 0.2.0
```

See `scripts/release.sh` (thin wrapper) and `xtask/` for details.

## Roadmap

See [ROADMAP.md](ROADMAP.md).

## Changelog

See [CHANGELOG.md](CHANGELOG.md).

## License

`pg_epanet` is licensed under the [MIT License](LICENSE).

The vendored [OWA-EPANET 2.3](vendor/epanet/) C toolkit is in the public domain — see [vendor/epanet/LICENSE](vendor/epanet/LICENSE).

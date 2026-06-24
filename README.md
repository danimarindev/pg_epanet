# pg_epanet

PostgreSQL extension (written in Rust via [pgrx](https://github.com/pgcentralfoundation/pgrx)) that parses EPANET `.inp` water network files and materialises them as queryable SQL tables with PostGIS geometry.

## Why pg_epanet?

Tools like WNTR or swmm-api parse an INP file into Python objects in memory. `pg_epanet` parses the INP **directly into PostgreSQL tables**: queryable with SQL, joinable with any other data, with PostGIS geometry ready to use as a GIS layer.

For users who already store infrastructure data in PostGIS this eliminates the intermediate pipeline of "export INP → Python script → re-import results". For bulk scenarios (importing thousands of INP files in parallel inside the database, e.g. resilience studies) the Rust implementation also provides a meaningful speed advantage.

## Features

- **Import** an EPANET INP file into permanent tables with a single function call
- **PostGIS geometry** generated automatically from `[COORDINATES]` and `[VERTICES]` sections
- **Hydraulic simulation** via the official OWA-EPANET 2.3 C toolkit (statically linked — no external installation required)
- **Simulation results** stored in queryable tables (`node_results`, `link_results`)
- Works on **managed PostgreSQL** (RDS, Supabase, etc.) — the INP is passed as `text`, no server filesystem access needed

## Requirements

- PostgreSQL 13–18
- [PostGIS](https://postgis.net/) extension
- Rust + [pgrx](https://github.com/pgcentralfoundation/pgrx) (for building from source)

## Quick start

```sql
-- 1. Enable required extensions
CREATE EXTENSION postgis;
CREATE EXTENSION pg_epanet;

-- 2. Import a network (pass the INP file content as text)
SELECT epanet_import('my_network', $inp$
[TITLE]
...
[END]
$inp$, 4326) AS network_id;

-- 3. Query the imported data
SELECT name, elevation, pressure, geom
FROM epanet.junctions
WHERE network_id = 1;

-- 4. Run a hydraulic simulation
SELECT epanet_simulate(1) AS run_id;

-- 5. Inspect results
SELECT node_id, round(pressure::numeric, 2) AS pressure_m
FROM epanet.node_results
WHERE run_id = 1
ORDER BY pressure ASC
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

Parses the INP and writes all sections to permanent tables under the `epanet` schema. Returns the `network_id`.

### Simulation

```sql
epanet_simulate(network_id int) → int
```

Runs a full hydraulic simulation using OWA-EPANET 2.3 and stores results in `epanet.node_results` and `epanet.link_results`. Returns the `run_id`.

### Table-returning functions (parse on the fly)

These functions parse the INP text in-query and return rows — useful for ad-hoc inspection without importing.

| Function | Returns |
|---|---|
| `epanet_junctions(inp_text)` | name, elevation, demand, pattern |
| `epanet_reservoirs(inp_text)` | name, head, pattern |
| `epanet_tanks(inp_text)` | name, elevation, levels, diameter, volume_curve |
| `epanet_pipes(inp_text)` | name, node1, node2, length, diameter, roughness, minor_loss, status |
| `epanet_pumps(inp_text)` | name, node1, node2, pump_type, head_curve, power, speed, pattern |
| `epanet_valves(inp_text)` | name, node1, node2, diameter, valve_type, setting, minor_loss |
| `epanet_coordinates(inp_text)` | node_id, x, y |
| `epanet_vertices(inp_text)` | link_id, x, y |

## Schema

After `epanet_import`, the `epanet` schema contains:

**Network catalogue**
- `networks` — one row per imported network (name, SRID, import timestamp, original INP text)

**Topology**
- `junctions`, `reservoirs`, `tanks` — nodes with `geom geometry(Point)`
- `pipes`, `pumps`, `valves` — links with `geom geometry(LineString)`
- `coordinates`, `vertices` — raw geometry data
- `nodes` — unified view of all node types

**Simulation results**
- `simulation_runs` — one row per simulation run
- `node_results` — head, pressure, demand per node per timestep
- `link_results` — flow, velocity, headloss per link per timestep

## Building from source

```bash
cargo install cargo-pgrx --version '=0.19.1'
cargo pgrx init
cargo pgrx run pg18   # compiles, starts sandbox, opens psql
```

Inside psql after code changes:
```sql
DROP EXTENSION pg_epanet;
CREATE EXTENSION pg_epanet;
```

Run the test suite:
```bash
cargo pgrx test pg18
```

## Roadmap

See [ROADMAP.md](ROADMAP.md).

## Changelog

See [CHANGELOG.md](CHANGELOG.md).

## License

MIT

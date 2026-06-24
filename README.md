# pg_epanet

PostgreSQL extension (written in Rust via [pgrx](https://github.com/pgcentralfoundation/pgrx)) that parses EPANET `.inp` water network files and materialises them as queryable SQL tables with PostGIS geometry.

> **Status:** [v0.1.0](https://github.com/danimarindev/pg_epanet/releases/tag/v0.1.0) released — see [CHANGELOG.md](CHANGELOG.md) for details.

## Why pg_epanet?

Tools like WNTR or swmm-api parse an INP file into Python objects in memory. `pg_epanet` parses the INP **directly into PostgreSQL tables**: queryable with SQL, joinable with any other data, with PostGIS geometry ready to use as a GIS layer.

For users who already store infrastructure data in PostGIS this eliminates the intermediate pipeline of "export INP → Python script → re-import results". For bulk scenarios (importing thousands of INP files in parallel inside the database, e.g. resilience studies) the Rust implementation also provides a meaningful speed advantage.

## Features

- **Import** an EPANET INP into permanent tables with a single function call
- **PostGIS geometry** — nodes as `Point`, pipes as `LineString` (with ordered `[VERTICES]`), pumps/valves as direct links
- **Hydraulic EPS simulation** via the official OWA-EPANET 2.3 C toolkit (statically linked — no external install)
- **Per-timestep results** stored in `node_results` and `link_results` (`step` column, full Extended Period Simulation)
- **Schema bootstrap** — all tables, indexes, and views created at `CREATE EXTENSION pg_epanet`
- **Delete** networks and all associated data with `epanet_delete`
- Works on **managed PostgreSQL** (RDS, Supabase, etc.) — the INP is passed as `text`, no server filesystem read required for import

### INP sections supported (v0.1.0)

Import and table-returning functions exist for: `[JUNCTIONS]`, `[RESERVOIRS]`, `[TANKS]`, `[PIPES]`, `[PUMPS]`, `[VALVES]`, `[COORDINATES]`, `[VERTICES]`.

Not yet imported: patterns, curves, options, times, controls, rules, sources, reactions, quality, and other metadata sections — see [ROADMAP.md](ROADMAP.md).

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
git checkout v0.1.0
cargo install cargo-pgrx --version '=0.19.1'
cargo pgrx init
cargo pgrx install --release --features pg18 --no-default-features --pg-config $(which pg_config)
```

Then in psql:

```sql
CREATE EXTENSION pg_epanet CASCADE;
```

Or use Docker (see above) for a ready-made PostgreSQL 18 + PostGIS + pg_epanet stack.

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

-- 5. Inspect results (filter by timestep if needed)
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

Runs a full Extended Period Simulation (EPS) using OWA-EPANET 2.3. Stores per-timestep results in `epanet.node_results` (head, pressure, demand) and `epanet.link_results` (flow, velocity, headloss). Returns the `run_id`. Fatal solver errors (code ≥ 100) abort the simulation; warning codes 1–99 are tolerated but not yet surfaced as PostgreSQL warnings.

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
| `epanet_vertices(inp_text)` | link_id, x, y |

## Schema

Created at `CREATE EXTENSION pg_epanet`:

**Network catalogue**
- `networks` — one row per imported network (name, SRID, import timestamp, original INP text)

**Topology**
- `junctions`, `reservoirs`, `tanks` — nodes with `geom geometry(Point)` and GiST index
- `pipes`, `pumps`, `valves` — links with `geom geometry(LineString)` and GiST index
- `coordinates`, `vertices` — raw geometry data (vertices ordered by `idx`)
- `nodes` — unified view of all node types

**Simulation results**
- `simulation_runs` — one row per simulation run (`n_steps` = timesteps solved)
- `node_results` — head, pressure, demand per node per `step`
- `link_results` — flow, velocity, headloss per link per `step`

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

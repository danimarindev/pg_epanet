# Roadmap

High-level plan for `pg_epanet`. Items within each milestone are roughly ordered by priority.

---

## v0.1.0 — First release ✅

**Released 2026-06-24.** Core topology import, PostGIS geometry, and hydraulic EPS simulation.

- [x] Generic INP section parser (`mod inp`) — engine-agnostic tokeniser
- [x] `CREATE EXTENSION pg_epanet` bootstraps the `epanet` schema, catalogue tables, result tables, GiST indexes, and `epanet.nodes` view
- [x] PostGIS dependency (`requires = 'postgis'`) with automatic install via `CASCADE`
- [x] `epanet_import(name, inp_text, srid)` — materialises INP sections into permanent tables
- [x] INP sections imported: `[JUNCTIONS]`, `[RESERVOIRS]`, `[TANKS]`, `[PIPES]`, `[PUMPS]`, `[VALVES]`, `[COORDINATES]`, `[VERTICES]`
- [x] Table-returning parse functions for all sections above (ad-hoc inspection without import)
- [x] PostGIS geometry: nodes → `Point`, pipes → `LineString` (with ordered vertices), pumps/valves → direct `LineString`
- [x] `epanet_delete(network_id)` — CASCADE delete of network, topology, and simulation results
- [x] `epanet_simulate(network_id)` — full EPS via OWA-EPANET 2.3 (`EN_openH / EN_runH / EN_nextH / EN_closeH`)
- [x] Per-timestep results in `node_results.step` and `link_results.step`; `simulation_runs.n_steps` reflects actual step count
- [x] Original INP stored verbatim in `networks.inp_text` for simulation re-use
- [x] Docker image + `docker-compose.yml` (Postgres 18 + PostGIS 3 + pre-installed extension)
- [x] Release tooling (`xtask/`) — Keep a Changelog, GitHub Releases
- [x] 35 tests (`cargo pgrx test pg18`) — 28 `#[pg_test]` + 7 pure-Rust parser unit tests

---

## v0.2.0 — INP completeness & simulation polish ✅

**Released 2026-06-24.** All EPANET metadata sections queryable in SQL; solver warnings surfaced to clients.

- [x] **Simulation warnings** — EPANET codes 1–99 emitted as PostgreSQL `WARNING` during `epanet_simulate`
- [x] `[PATTERNS]` — demand/time multiplier curves (`patterns` table, indexed by `idx`)
- [x] `[CURVES]` — head/volume/pump curves (`curves` table, indexed by `idx`)
- [x] `[OPTIONS]` — simulation options (key-value `options` table)
- [x] `[TIMES]` — duration and timestep settings (`times` table)
- [x] `[CONTROLS]` / `[RULES]` — stored as full rule text in `controls` and `rules` tables
- [x] `[SOURCES]` / `[REACTIONS]` / `[QUALITY]` — prerequisites for water quality (v0.3)
- [x] `[EMITTERS]`, `[DEMANDS]`, `[STATUS]`, `[ENERGY]`, `[REPORT]` — imported with table-returning functions
- [x] `src/epanet_sections.rs` — multi-line parsers (patterns, curves, rules blocks)
- [x] 44 tests total (`cargo pgrx test pg18`)

**Known gaps after v0.2.0:**

- Simulation writes temporary `.inp/.rpt/.out` files under `/tmp` on the Postgres server
- `epanet_simulate` reads `inp_text` verbatim — metadata tables not yet used to reconstruct INP
- Structured parse of CONTROLS/RULES into columns — backlog for `epanet_export` (v0.4)

---

## v0.3 — Water quality

**Goal:** extend simulation to water quality (WQ) using EPANET's `EN_solveQ` / `EN_runQ` / `EN_nextQ`.

- [ ] `epanet_simulate_quality(network_id, run_id)` — runs WQ on top of an existing hydraulic run
- [ ] `epanet.node_quality_results` — chlorine / age / trace concentration per node per timestep
- [ ] `epanet.link_quality_results` — average quality per link per timestep

---

## v0.4 — Bulk workflows & convenience

**Goal:** make multi-network and multi-scenario workflows ergonomic.

- [ ] `epanet_import_file(network_name, file_path, srid)` — optional server-side file read (superuser only) for local deployments
- [ ] `epanet_export(network_id) → text` — regenerate a valid INP from stored tables
- [ ] Idempotent import: `epanet_import(..., replace := true)` to update an existing network by name
- [ ] Performance: batch geometry updates (currently one `UPDATE` per node/link type during import)

---

## v0.5 — SWMM

**Goal:** extend the same architecture to SWMM stormwater networks using the shared section tokeniser.

- [ ] `swmm_import(name, inp_text, srid)` — parse SWMM `.inp` files
- [ ] SWMM node tables: junctions, outfalls, storage
- [ ] SWMM link tables: conduits, weirs, orifices, pumps
- [ ] Table-returning functions: `swmm_junctions()`, `swmm_conduits()`, etc.
- [ ] Optional: hydraulic/hydrology simulation via OWA-SWMM C toolkit

---

## Backlog / ideas

Not yet scheduled into a milestone:

- **Scenario management** — store multiple parameter sets (demands, roughness) for the same topology; run comparative simulations
- **pg_epanet_admin** — helper functions to list networks, runs, disk usage
- **Partitioning** — partition `node_results` and `link_results` by `run_id` for very large EPS runs
- **Binary result files** — attach EPANET's `.out` file as a `bytea` column for downstream tools
- **Packaging** — pre-built binaries / PGXN package for installation without a Rust toolchain
- **Simulation without `/tmp`** — in-memory or `bytea`-backed EPANET project open (relevant for locked-down managed Postgres)
- **PostgreSQL 19** — pgrx feature flag exists; track compatibility as PG19 stabilises

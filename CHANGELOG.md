# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.6.2] — 2026-06-29

### Added
- **`epanet.links`** view — unified pipes/pumps/valves with `link_type` and `geom`.
- **Map editing:** `epanet_set_node_coordinates`, `epanet_set_node_geom` — move nodes and cascade link geometry refresh.
- **Geometry-native adds:** `epanet_add_junction_geom`, `epanet_add_reservoir_geom`, `epanet_add_tank_geom`, `epanet_add_pipe_geom` (PostGIS `geometry` args).
- **Pipe shape:** `epanet_apply_pipe_shape(network_id, pipe, wkt)` — LineString → vertices + geom.
- **Scenario map layer:** `scenario_elements.geom` column; `epanet_scenario_nodes(scenario_id)`, `epanet_scenario_links(scenario_id)` for map preview with provisional flag.
- **Scenario editing:** `epanet_set_scenario_node_coordinates`, `epanet_set_scenario_node_geom`, `epanet_add_scenario_vertex`, `epanet_refresh_scenario_geoms`.
- Helper `epanet.effective_node_xy(scenario_id, node_id)` for overlay coordinate resolution.
- Module `src/map.rs`; migration `sql/pg_epanet--0.6.1--0.6.2.sql`.
- 3 new `#[pg_test]` cases for move-node, links view, and scenario map geometry.

## [0.6.1] — 2026-06-29

### Added
- **`epanet_create_network(name, srid)`** — empty network shell with default OPTIONS/TIMES/REPORT.
- **Base topology:** `epanet_add_reservoir`, `epanet_add_tank`, `epanet_add_pump`, `epanet_add_valve`.
- **Scenario topology:** `epanet_add_scenario_reservoir`, `epanet_add_scenario_tank`, `epanet_add_scenario_pump`, `epanet_add_scenario_valve`.
- **Metadata builder:** `epanet_add_pattern`, `epanet_add_curve`, `epanet_set_option`, `epanet_set_times`, `epanet_set_report`, `epanet_set_reactions`, `epanet_set_quality`, `epanet_set_energy`, `epanet_add_control`, `epanet_add_rule`, `epanet_add_demand`, `epanet_add_emitter`, `epanet_set_link_status`, `epanet_add_source`, `epanet_add_vertex`.
- **`epanet_merge_scenario_into_base`** now promotes reservoirs, tanks, pumps, and valves.
- Module `src/builder.rs`; migration `sql/pg_epanet--0.6.0--0.6.1.sql`.
- `#[pg_test]` building and simulating a network from scratch.

## [0.6.0] — 2026-06-29

### Added
- **`epanet.scenario_elements`** and **`epanet.scenario_element_vertices`** — provisional topology visible only in scenario simulations.
- **Base network editing:** `epanet_add_junction`, `epanet_add_pipe`, `epanet_remove_element`, `epanet_connect_nodes`.
- **Scenario topology:** `epanet_add_scenario_junction`, `epanet_add_scenario_pipe`, `epanet_remove_scenario_element`.
- **`epanet_merge_scenario_into_base(scenario_id)`** — promotes scenario elements + overrides into base tables and refreshes INP.
- Scenario overlay engine applies provisional elements to effective INP at simulate time.
- Module `src/topology.rs`; migration `sql/pg_epanet--0.5.0--0.6.0.sql`.
- 2 new `#[pg_test]` cases for base and scenario topology.

## [0.5.0] — 2026-06-29

### Added
- **Scenario model** — `epanet.scenarios` and `epanet.scenario_overrides` tables; base `networks.inp_text` is never modified for what-if studies.
- `epanet_create_scenario`, `epanet_set_scenario_override`, `epanet_delete_scenario`.
- `epanet_simulate_scenario(scenario_id)` — builds effective INP in memory (base INP + overrides) and runs EPS.
- `epanet_compare_runs(run_id_a, run_id_b)` — node pressure and link flow deltas per timestep.
- `epanet_scenario_pipe_closure(network_id, name, pipe_id)` — pipe-break / criticality convenience.
- `epanet_scenario_fire_flow(network_id, name, junction_id, required_flow)` — fire-flow demand override.
- `simulation_runs.scenario_id` — links runs to the scenario that produced them.
- Module `src/scenario.rs`; `inp::render_sections` for INP overlay serialization.

### Changed
- **`epanet_simulate`** uses the immutable imported INP snapshot — no longer auto-syncs from SQL tables or overwrites `inp_text`.
- **`epanet_simulate_quality`** re-applies the same scenario overlay as the hydraulic run when `scenario_id` is set.
- Use **`epanet_refresh_inp`** explicitly when you want to persist table edits to `inp_text`; use **scenarios** for simulation parameter changes.

## [0.4.0] — 2026-06-29

### Added
- `epanet_export(network_id)` — regenerate EPANET INP text from stored tables.
- `epanet_refresh_inp(network_id)` — sync `networks.inp_text` from table state.
- `epanet_validate(network_id)` — table-returning topology/reference checks (missing nodes, dangling patterns/curves, orphans, disconnected components).
- `epanet_import(..., replace := false)` — when `replace` is true, deletes existing networks with the same name before import.
- `epanet_import_file(name, path, srid, replace)` — superuser server-side INP file import.
- GUC `pg_epanet.temp_dir` — configurable directory for simulation temp files (defaults to `TMPDIR` or `/tmp`).

### Changed
- `epanet_simulate` and `epanet_simulate_quality` rebuild INP from SQL tables before running, so edits to demands, status, options, etc. take effect automatically.
- Simulation temp files use `pg_epanet.temp_dir` instead of hard-coded `/tmp`.
- Import geometry updates batched into fewer SQL statements for junctions/tanks/reservoirs and pumps/valves.

## [0.3.0] — 2026-06-29

### Added
- FFI bindings for EPANET water quality EPS: `EN_openQ`, `EN_initQ`, `EN_runQ`, `EN_nextQ`, `EN_closeQ`.
- `epanet_simulate_quality(network_id, run_id)` — runs water quality EPS for an existing hydraulic run; stores results in `epanet.node_quality_results` and `epanet.link_quality_results`.
- `epanet.node_quality_results` — concentration / water age / trace per node per timestep.
- `epanet.link_quality_results` — average link quality per timestep.
- Indexes on `(run_id)` and `(run_id, step)` for quality result tables.
- View `epanet.node_quality_envelope` — min/max/avg quality per node per run.
- `epanet_count_nodes_below_threshold(run_id, threshold)` — count nodes whose minimum quality falls below a threshold.
- 2 new `#[pg_test]` cases for quality schema and simulation.

## [0.2.1] — 2026-06-25

### Added
- B-tree indexes on `(network_id, node1)` and `(network_id, node2)` for `pipes`, `pumps`, and `valves` — faster graph traversal and incident-link lookups.
- B-tree index on `simulation_runs(network_id)` — list simulation runs per network.
- `#[pg_test]` verifying topology and simulation-run indexes exist at install time.

### Changed
- README: usage guide (import → query → simulate → results), upgrade notes, and performance/index reference table.

## [0.2.0] — 2026-06-25

### Added
- Import and table-returning functions for all remaining EPANET metadata sections: `[PATTERNS]`, `[CURVES]`, `[OPTIONS]`, `[TIMES]`, `[CONTROLS]`, `[RULES]`, `[DEMANDS]`, `[EMITTERS]`, `[STATUS]`, `[SOURCES]`, `[REACTIONS]`, `[QUALITY]`, `[ENERGY]`, `[REPORT]`.
- New `epanet` schema tables for metadata sections (all with `network_id` FK and `ON DELETE CASCADE`).
- `src/epanet_sections.rs` — pure-Rust parsers for multi-line sections (patterns, curves, rules blocks) and key-value sections.
- Table-returning functions: `epanet_patterns`, `epanet_curves`, `epanet_options`, `epanet_times`, `epanet_controls`, `epanet_rules`, `epanet_demands`, `epanet_emitters`, `epanet_status`, `epanet_sources`, `epanet_reactions`, `epanet_quality`, `epanet_energy`, `epanet_report`.
- `epanet_import` now materialises all metadata sections alongside topology.
- 9 new `#[pg_test]` cases and 4 unit tests in `epanet_sections` (44 tests total).

### Changed
- `epanet_simulate` emits PostgreSQL `WARNING` messages for EPANET solver codes 1–99 (pump out of range, unbalanced network, etc.), including timestep and error text.
- Extended `tests/fixtures/simple.inp` with all metadata sections for integration testing.

## [0.1.0] — 2026-06-24

### Added
- `epanet` schema and all catalogue/result tables created at `CREATE EXTENSION pg_epanet` time (via `extension_sql!` bootstrap).
- PostGIS declared as an extension dependency (`requires = 'postgis'`); `CREATE EXTENSION pg_epanet CASCADE` installs it automatically.
- Docker image (`postgres:18-trixie` + PostGIS 3 from PGDG) and `docker-compose.yml` for local use.
- `epanet_import(name, inp_text, srid)` — parses an EPANET INP file and materialises all sections into permanent tables under the `epanet` schema.
- `epanet_delete(network_id int) → boolean` — removes a network and all associated topology and simulation results via CASCADE.
- GiST spatial indexes on all `geom` columns (junctions, reservoirs, tanks, pipes, pumps, valves).
- Table-returning functions for all major INP sections: `epanet_junctions`, `epanet_reservoirs`, `epanet_tanks`, `epanet_pipes`, `epanet_pumps`, `epanet_valves`, `epanet_coordinates`, `epanet_vertices`.
- PostGIS geometry generation:
  - Nodes (junctions, tanks, reservoirs) → `geometry(Point)` from `[COORDINATES]`.
  - Pipes → `geometry(LineString)` from node endpoints + intermediate `[VERTICES]`, preserving vertex order.
  - Pumps and valves → direct node1→node2 `geometry(LineString)`.
- `epanet.nodes` — unified view of all node types (junctions, tanks, reservoirs).
- `epanet_simulate(network_id int) → int` — full Extended Period Simulation (EPS) using the official OWA-EPANET 2.3 C toolkit. Stores per-timestep results in `epanet.node_results` (head, pressure, demand) and `epanet.link_results` (flow, velocity, headloss). Returns the `run_id`.
- Result tables (`epanet.simulation_runs`, `epanet.node_results`, `epanet.link_results`) created at extension install time.
- `inp_text TEXT` column on `epanet.networks` — the original INP is stored verbatim for simulation re-use.
- Generic INP parser (`mod inp`) — tokenises sections and fields; engine-agnostic, ready for SWMM reuse.
- 35 unit tests (`cargo pgrx test pg18`) covering parsing edge cases, import/delete, and spatial indexes.
- Tested with a real 1,152-node / 1,165-pipe Costa Rica distribution network (351 KB INP, EPSG:5367), producing 97 EPS timesteps × 1,152 nodes = 111,744 result rows.
- Release tooling in Rust (`xtask/`): Keep a Changelog parsing, GitHub Release notes, full release workflow.

### Changed
- OWA-EPANET 2.3 C source vendored directly into `vendor/epanet/` with a hand-written `build.rs` and `src/ffi.rs`. Removed dependency on the `epanet-sys` crate.
- EPS hydraulic solver loop (`EN_openH / EN_initH / EN_runH / EN_nextH / EN_closeH`) replaces the single-shot `EN_solveH`. `simulation_runs.n_steps` now reflects the actual number of timesteps solved.
- Solver warning codes 1–99 (pump out of range, unbalanced network, etc.) are non-fatal; only codes ≥ 100 abort the simulation.
- `EN_open` error code 200 (formatting warnings) treated as non-fatal.

### Notes
- When loading large INP files via psql `\COPY`, always use `ORDER BY lineno` with a `SERIAL` column — `ORDER BY ctid` does not guarantee insertion order for large files.
- First packaged release; future versions upgrade via `ALTER EXTENSION pg_epanet UPDATE`.

[unreleased]: https://github.com/danimarindev/pg_epanet/compare/v0.6.0...main
[0.6.0]: https://github.com/danimarindev/pg_epanet/compare/v0.5.0...v0.6.0
[0.5.0]: https://github.com/danimarindev/pg_epanet/compare/v0.4.0...v0.5.0
[0.4.0]: https://github.com/danimarindev/pg_epanet/compare/v0.3.0...v0.4.0
[0.3.0]: https://github.com/danimarindev/pg_epanet/compare/v0.2.1...v0.3.0
[0.2.1]: https://github.com/danimarindev/pg_epanet/compare/v0.2.0...v0.2.1
[0.2.0]: https://github.com/danimarindev/pg_epanet/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/danimarindev/pg_epanet/releases/tag/v0.1.0

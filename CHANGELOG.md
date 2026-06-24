# Changelog

All notable changes to `pg_epanet` are documented here.  
Format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

---

## [Unreleased]

### Added
- `epanet_import(name, inp_text, srid)` — parses an EPANET INP file and materialises all sections into permanent tables under the `epanet` schema.
- Table-returning functions for all major INP sections: `epanet_junctions`, `epanet_reservoirs`, `epanet_tanks`, `epanet_pipes`, `epanet_pumps`, `epanet_valves`, `epanet_coordinates`, `epanet_vertices`.
- PostGIS geometry generation:
  - Nodes (junctions, tanks, reservoirs) → `geometry(Point)` from `[COORDINATES]`.
  - Pipes → `geometry(LineString)` from node endpoints + intermediate `[VERTICES]`, preserving vertex order.
  - Pumps and valves → direct node1→node2 `geometry(LineString)`.
- `epanet.nodes` — unified view of all node types (junctions, tanks, reservoirs).
- `epanet_simulate(network_id int) → int` — full Extended Period Simulation (EPS) using the official OWA-EPANET 2.3 C toolkit. Stores per-timestep results in `epanet.node_results` (head, pressure, demand) and `epanet.link_results` (flow, velocity, headloss). Returns the `run_id`.
- Result tables created automatically by `epanet_import`: `epanet.simulation_runs`, `epanet.node_results`, `epanet.link_results`.
- `inp_text TEXT` column on `epanet.networks` — the original INP is stored verbatim for simulation re-use.
- Generic INP parser (`mod inp`) — tokenises sections and fields; engine-agnostic, ready for SWMM reuse.
- 33 unit tests (`cargo pgrx test pg18`) covering parsing edge cases: optional fields, default values, `*` as NULL, case normalisation, empty sections, etc.
- Tested with a real 1,152-node / 1,165-pipe Costa Rica distribution network (351 KB INP, EPSG:5367), producing 97 EPS timesteps × 1,152 nodes = 111,744 result rows.

### Changed
- OWA-EPANET 2.3 C source vendored directly into `vendor/epanet/` with a hand-written `build.rs` and `src/ffi.rs`. Removed dependency on the `epanet-sys` crate.
- EPS hydraulic solver loop (`EN_openH / EN_initH / EN_runH / EN_nextH / EN_closeH`) replaces the single-shot `EN_solveH`. `simulation_runs.n_steps` now reflects the actual number of timesteps solved.
- Solver warning codes 1–99 (pump out of range, unbalanced network, etc.) are non-fatal; only codes ≥ 100 abort the simulation.
- `EN_open` error code 200 (formatting warnings) treated as non-fatal.

### Notes
- When loading large INP files via psql `\COPY`, always use `ORDER BY lineno` with a `SERIAL` column — `ORDER BY ctid` does not guarantee insertion order for large files.
- Upgrade scripts (`ALTER EXTENSION pg_epanet UPDATE`) are not yet provided. The first versioned release will include them.

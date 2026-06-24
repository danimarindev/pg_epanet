# Changelog

All notable changes to `pg_epanet` are documented here.  
Format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

---

## [Unreleased]

---

## [0.2.0] — 2026-06-24

### Added
- **Hydraulic simulation** (`epanet_simulate(network_id int) → int`) using the official OWA-EPANET 2.3 C toolkit, statically linked via `epanet-sys 2.3`.
- Result tables: `epanet.simulation_runs`, `epanet.node_results` (head, pressure, demand), `epanet.link_results` (flow, velocity, headloss).
- Result tables are created automatically by `epanet_import` (no separate setup needed).
- Support for pumps: `epanet_pumps()` table-returning function and `epanet.pumps` permanent table with PostGIS LineString geometry.
- Support for valves: `epanet_valves()` table-returning function and `epanet.valves` permanent table with PostGIS LineString geometry.
- `epanet.nodes` — unified view of all node types (junctions, tanks, reservoirs).
- `inp_text TEXT` column on `epanet.networks` — the original INP is stored verbatim for re-use in simulation.

### Fixed
- `EN_open` error code 200 (formatting warnings) is now treated as non-fatal; OWA-EPANET keeps the project open and usable in this case.
- Bulk INSERT approach for simulation results (single `VALUES` clause per table) for performance.

### Notes
- Simulation is currently single-step (step=0). Full EPS multi-timestep support is planned for 0.3.0.
- When loading large INP files via psql `\COPY`, always use `ORDER BY lineno` with a `SERIAL` column — `ORDER BY ctid` does not guarantee insertion order for large files.

---

## [0.1.0] — 2026-06-23

### Added
- `epanet_import(name, inp_text, srid)` — parses an EPANET INP file and materialises all sections into permanent tables under the `epanet` schema.
- Table-returning functions for all major INP sections: `epanet_junctions`, `epanet_reservoirs`, `epanet_tanks`, `epanet_pipes`, `epanet_coordinates`, `epanet_vertices`.
- PostGIS geometry generation:
  - Nodes (junctions, tanks, reservoirs) → `geometry(Point)` from `[COORDINATES]`.
  - Pipes → `geometry(LineString)` from node endpoints + intermediate `[VERTICES]`, preserving vertex order.
  - Pumps and valves → direct node1→node2 `geometry(LineString)`.
- Support for all standard INP sections: `[JUNCTIONS]`, `[RESERVOIRS]`, `[TANKS]`, `[PIPES]`, `[PUMPS]`, `[VALVES]`, `[COORDINATES]`, `[VERTICES]`.
- Generic INP parser (`mod inp`) — tokenises sections and fields; engine-agnostic, ready for SWMM reuse.
- 33 unit tests (`cargo pgrx test pg18`) covering parsing edge cases: optional fields, default values, `*` as NULL, case normalisation, empty sections, etc.
- Tested with a real 1,152-node / 1,165-pipe Costa Rica distribution network (351 KB INP, CRTM05 EPSG:5367).

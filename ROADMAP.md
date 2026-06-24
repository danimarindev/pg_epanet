# Roadmap

High-level plan for `pg_epanet`. Items within each milestone are roughly ordered by priority.

---

## v0.3 — Simulation completa (EPS)

**Goal:** make `epanet_simulate` production-ready with full Extended Period Simulation support.

- [ ] **EPS multi-timestep** — replace the current single-step solve with the `EN_openH / EN_runH / EN_nextH / EN_closeH` loop; store results per timestep in `node_results.step` and `link_results.step`.
- [ ] **Update `simulation_runs.n_steps`** with the actual number of timesteps solved.
- [ ] **Simulation warnings** — surface EPANET warning codes (e.g. unbalanced network, negative pressures) as PostgreSQL `WARNING` messages rather than silently ignoring them.
- [ ] **`epanet_delete(network_id int)`** — convenience function to remove a network and all its results via CASCADE.
- [ ] GiST spatial indexes on all `geom` columns (currently missing; important for large networks).
- [ ] Suppress spurious `NOTICE: column already exists` messages from `create_epanet_schema`.

---

## v0.4 — Calidad del agua

**Goal:** extend simulation to water quality (WQ) using EPANET's `EN_solveQ` / `EN_runQ` / `EN_nextQ`.

- [ ] `epanet_simulate_quality(network_id, run_id)` — runs a water quality simulation on top of an existing hydraulic run.
- [ ] `epanet.node_quality_results` — chlorine / age / trace concentration per node per timestep.
- [ ] `epanet.link_quality_results` — average quality per link per timestep.
- [ ] Support for `[QUALITY]`, `[SOURCES]`, `[REACTIONS]` INP sections in the importer.

---

## v0.5 — Importación masiva y conveniencia

**Goal:** make bulk workflows (many networks, many scenarios) ergonomic.

- [ ] `epanet_import_file(network_name, file_path, srid)` — optional server-side file read (superuser only) for local deployments where filesystem access is acceptable.
- [ ] `epanet_export(network_id) → text` — regenerate a valid INP from the stored tables.
- [ ] Idempotent import option: `epanet_import(..., replace := true)` to update an existing network by name instead of always inserting a new row.
- [ ] Performance: batch-insert geometry updates (currently one UPDATE per node type).

---

## v0.6 — SWMM

**Goal:** extend the same architecture to SWMM stormwater networks.

- [ ] `swmm_import(name, inp_text, srid)` — parse SWMM `.inp` files using the existing generic section tokeniser (`mod inp`).
- [ ] SWMM node tables: `epanet.swmm_junctions`, `swmm_outfalls`, `swmm_storage`.
- [ ] SWMM link tables: `swmm_conduits`, `swmm_weirs`, `swmm_orifices`, `swmm_pumps`.
- [ ] Table-returning functions: `swmm_junctions()`, `swmm_conduits()`, etc.
- [ ] Optional: hydraulic/hydrology simulation via OWA-SWMM C toolkit.

---

## Backlog / ideas

These are not yet scheduled into a milestone:

- **Scenario management** — store multiple parameter sets (demands, roughness) for the same topology, run comparative simulations.
- **pg_epanet_admin** — helper functions to list networks, runs, disk usage.
- **Partitioning** — partition `node_results` and `link_results` by `run_id` for very large EPS runs.
- **Binary result files** — option to keep EPANET's binary output file (`.out`) attached as a `bytea` column for downstream tools that read it directly.
- **Packaging** — `.pgxn` package for easy installation without Rust toolchain; pre-built binaries for common platforms.
- **PostgreSQL 19+** — track pgrx compatibility as new major versions are released.

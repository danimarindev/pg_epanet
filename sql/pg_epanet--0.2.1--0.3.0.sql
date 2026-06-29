-- Upgrade pg_epanet 0.2.1 → 0.3.0: water quality simulation results

CREATE TABLE epanet.node_quality_results (
    run_id   INT NOT NULL REFERENCES epanet.simulation_runs(id) ON DELETE CASCADE,
    step     INT NOT NULL,
    node_id  TEXT NOT NULL,
    quality  DOUBLE PRECISION,
    PRIMARY KEY (run_id, step, node_id)
);

CREATE INDEX node_quality_results_run ON epanet.node_quality_results(run_id);
CREATE INDEX node_quality_results_run_step ON epanet.node_quality_results(run_id, step);

CREATE TABLE epanet.link_quality_results (
    run_id    INT NOT NULL REFERENCES epanet.simulation_runs(id) ON DELETE CASCADE,
    step      INT NOT NULL,
    link_id   TEXT NOT NULL,
    quality   DOUBLE PRECISION,
    PRIMARY KEY (run_id, step, link_id)
);

CREATE INDEX link_quality_results_run ON epanet.link_quality_results(run_id);
CREATE INDEX link_quality_results_run_step ON epanet.link_quality_results(run_id, step);

CREATE VIEW epanet.node_quality_envelope AS
SELECT
    run_id,
    node_id,
    min(quality) AS min_quality,
    max(quality) AS max_quality,
    avg(quality) AS avg_quality
FROM epanet.node_quality_results
WHERE quality IS NOT NULL
GROUP BY run_id, node_id;

-- Upgrade pg_epanet 0.4.0 → 0.5.0: scenarios and comparative runs

CREATE TABLE epanet.scenarios (
    id                 SERIAL PRIMARY KEY,
    network_id         INT NOT NULL REFERENCES epanet.networks(id) ON DELETE CASCADE,
    name               TEXT NOT NULL,
    description        TEXT,
    demand_multiplier  FLOAT8 NOT NULL DEFAULT 1.0,
    created_at         TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (network_id, name)
);

CREATE INDEX scenarios_network ON epanet.scenarios(network_id);

CREATE TABLE epanet.scenario_overrides (
    scenario_id  INT NOT NULL REFERENCES epanet.scenarios(id) ON DELETE CASCADE,
    target_type  TEXT NOT NULL,
    target_id    TEXT NOT NULL,
    parameter    TEXT NOT NULL,
    value        TEXT NOT NULL,
    PRIMARY KEY (scenario_id, target_type, target_id, parameter)
);

CREATE INDEX scenario_overrides_scenario ON epanet.scenario_overrides(scenario_id);

ALTER TABLE epanet.simulation_runs
    ADD COLUMN scenario_id INT REFERENCES epanet.scenarios(id) ON DELETE SET NULL;

CREATE INDEX simulation_runs_scenario ON epanet.simulation_runs(scenario_id);

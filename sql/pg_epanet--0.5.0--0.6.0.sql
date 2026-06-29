-- Upgrade pg_epanet 0.5.0 → 0.6.0: scenario-scoped topology elements

CREATE TABLE epanet.scenario_elements (
    scenario_id  INT NOT NULL REFERENCES epanet.scenarios(id) ON DELETE CASCADE,
    element_type TEXT NOT NULL,
    name         TEXT NOT NULL,
    inp_fields   TEXT NOT NULL,
    coord_x      FLOAT8,
    coord_y      FLOAT8,
    PRIMARY KEY (scenario_id, element_type, name)
);

CREATE INDEX scenario_elements_scenario ON epanet.scenario_elements(scenario_id);

CREATE TABLE epanet.scenario_element_vertices (
    scenario_id INT NOT NULL REFERENCES epanet.scenarios(id) ON DELETE CASCADE,
    link_id     TEXT NOT NULL,
    idx         INT NOT NULL,
    x           FLOAT8 NOT NULL,
    y           FLOAT8 NOT NULL,
    PRIMARY KEY (scenario_id, link_id, idx)
);

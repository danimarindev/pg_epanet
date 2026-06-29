-- Upgrade pg_epanet 0.6.1 → 0.6.2: map-editing layer (links view, scenario geom, geom APIs)

CREATE VIEW epanet.links AS
    SELECT network_id, name AS link_id, 'pipe'::text AS link_type, node1, node2, geom
      FROM epanet.pipes
    UNION ALL
    SELECT network_id, name, 'pump',  node1, node2, geom FROM epanet.pumps
    UNION ALL
    SELECT network_id, name, 'valve', node1, node2, geom FROM epanet.valves;

ALTER TABLE epanet.scenario_elements ADD COLUMN IF NOT EXISTS geom geometry;
CREATE INDEX IF NOT EXISTS scenario_elements_geom ON epanet.scenario_elements USING GIST (geom);

UPDATE epanet.scenario_elements se
SET geom = ST_SetSRID(ST_MakePoint(se.coord_x, se.coord_y), n.srid)
FROM epanet.scenarios s
JOIN epanet.networks n ON n.id = s.network_id
WHERE se.scenario_id = s.id
  AND se.coord_x IS NOT NULL AND se.coord_y IS NOT NULL
  AND se.element_type IN ('junction', 'reservoir', 'tank')
  AND se.geom IS NULL;

CREATE OR REPLACE FUNCTION epanet.effective_node_xy(p_scenario_id int, p_node_id text)
RETURNS TABLE(x float8, y float8)
LANGUAGE sql STABLE AS $$
    SELECT COALESCE(se.coord_x, c.x) AS x, COALESCE(se.coord_y, c.y) AS y
    FROM epanet.scenarios s
    LEFT JOIN epanet.scenario_elements se
      ON se.scenario_id = s.id AND se.name = p_node_id
     AND se.element_type IN ('junction', 'reservoir', 'tank')
    LEFT JOIN epanet.coordinates c
      ON c.network_id = s.network_id AND c.node_id = p_node_id
    WHERE s.id = p_scenario_id
      AND COALESCE(se.coord_x, c.x) IS NOT NULL
$$;

CREATE OR REPLACE FUNCTION epanet_scenario_nodes(p_scenario_id int)
RETURNS TABLE(
    node_id text,
    node_type text,
    provisional boolean,
    geom geometry
)
LANGUAGE sql STABLE AS $$
    WITH s AS (SELECT network_id FROM epanet.scenarios WHERE id = p_scenario_id)
    SELECT n.node_id, n.node_type, false AS provisional, n.geom
    FROM epanet.nodes n, s
    WHERE n.network_id = s.network_id
      AND NOT EXISTS (
          SELECT 1 FROM epanet.scenario_elements se
          WHERE se.scenario_id = p_scenario_id AND se.name = n.node_id
            AND se.element_type IN ('junction', 'reservoir', 'tank')
      )
    UNION ALL
    SELECT se.name,
           se.element_type,
           true,
           COALESCE(se.geom, ST_SetSRID(ST_MakePoint(se.coord_x, se.coord_y), net.srid))
    FROM epanet.scenario_elements se
    JOIN epanet.scenarios sc ON sc.id = se.scenario_id
    JOIN epanet.networks net ON net.id = sc.network_id
    WHERE se.scenario_id = p_scenario_id
      AND se.element_type IN ('junction', 'reservoir', 'tank')
$$;

CREATE OR REPLACE FUNCTION epanet_scenario_links(p_scenario_id int)
RETURNS TABLE(
    link_id text,
    link_type text,
    node1 text,
    node2 text,
    provisional boolean,
    geom geometry
)
LANGUAGE sql STABLE AS $$
    WITH s AS (SELECT network_id FROM epanet.scenarios WHERE id = p_scenario_id)
    SELECT l.link_id, l.link_type, l.node1, l.node2, false, l.geom
    FROM epanet.links l, s
    WHERE l.network_id = s.network_id
      AND NOT EXISTS (
          SELECT 1 FROM epanet.scenario_elements se
          WHERE se.scenario_id = p_scenario_id AND se.name = l.link_id
            AND se.element_type IN ('pipe', 'pump', 'valve')
      )
    UNION ALL
    SELECT se.name,
           se.element_type,
           split_part(se.inp_fields, ' ', 1),
           split_part(se.inp_fields, ' ', 2),
           true,
           se.geom
    FROM epanet.scenario_elements se
    WHERE se.scenario_id = p_scenario_id
      AND se.element_type IN ('pipe', 'pump', 'valve')
$$;

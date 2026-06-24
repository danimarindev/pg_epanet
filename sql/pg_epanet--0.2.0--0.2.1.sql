-- pg_epanet upgrade: 0.2.0 → 0.2.1
-- Performance indexes for topology traversal and simulation lookup.

CREATE INDEX IF NOT EXISTS simulation_runs_network ON epanet.simulation_runs(network_id);

CREATE INDEX IF NOT EXISTS pipes_node1  ON epanet.pipes  (network_id, node1);
CREATE INDEX IF NOT EXISTS pipes_node2  ON epanet.pipes  (network_id, node2);
CREATE INDEX IF NOT EXISTS pumps_node1  ON epanet.pumps  (network_id, node1);
CREATE INDEX IF NOT EXISTS pumps_node2  ON epanet.pumps  (network_id, node2);
CREATE INDEX IF NOT EXISTS valves_node1 ON epanet.valves (network_id, node1);
CREATE INDEX IF NOT EXISTS valves_node2 ON epanet.valves (network_id, node2);

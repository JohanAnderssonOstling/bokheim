BEGIN IMMEDIATE;

UPDATE concept SET preferred_label = 'Industry & Industrial Organization'
WHERE concept_id = 'unified:6cd36c31-3f37-5ae2-af25-b27360c7b523';

DELETE FROM source_selector
WHERE concept_id = 'unified:6cd36c31-3f37-5ae2-af25-b27360c7b523';

INSERT OR IGNORE INTO concept (concept_id, preferred_label) VALUES
  ('bokheim:industry:general', 'Industry — General'),
  ('bokheim:industry:industrialization', 'Industrialization'),
  ('bokheim:industry:rural', 'Rural Industries'),
  ('bokheim:industry:sweatshops', 'Sweatshops'),
  ('bokheim:industry:large', 'Large Industry & Factory System'),
  ('bokheim:industry:contracting', 'Industrial Contracting'),
  ('bokheim:industry:trade-associations', 'Trade Associations'),
  ('bokheim:industry:corporations', 'Corporations'),
  ('bokheim:industry:cooperatives', 'Cooperative Enterprises'),
  ('bokheim:industry:policy', 'Industrial Policy');

INSERT OR IGNORE INTO concept_parent
  (concept_id, parent_concept_id, ordinal, display_order) VALUES
  ('bokheim:industry:general', 'unified:6cd36c31-3f37-5ae2-af25-b27360c7b523', 0, 10),
  ('bokheim:industry:industrialization', 'unified:6cd36c31-3f37-5ae2-af25-b27360c7b523', 0, 20),
  ('bokheim:industry:rural', 'unified:6cd36c31-3f37-5ae2-af25-b27360c7b523', 0, 30),
  ('unified:f7738f96-847e-5fa8-b686-8b1ffd53cb85', 'unified:6cd36c31-3f37-5ae2-af25-b27360c7b523', 1, 40),
  ('bokheim:industry:sweatshops', 'unified:6cd36c31-3f37-5ae2-af25-b27360c7b523', 0, 50),
  ('unified:087cc6ae-5357-5c3c-a220-58af778dd88a', 'unified:6cd36c31-3f37-5ae2-af25-b27360c7b523', 1, 60),
  ('bokheim:industry:large', 'unified:6cd36c31-3f37-5ae2-af25-b27360c7b523', 0, 70),
  ('bokheim:industry:contracting', 'unified:6cd36c31-3f37-5ae2-af25-b27360c7b523', 0, 80),
  ('bokheim:industry:trade-associations', 'unified:6cd36c31-3f37-5ae2-af25-b27360c7b523', 0, 90),
  ('bokheim:industry:corporations', 'unified:6cd36c31-3f37-5ae2-af25-b27360c7b523', 0, 100),
  ('bokheim:industry:cooperatives', 'unified:6cd36c31-3f37-5ae2-af25-b27360c7b523', 0, 110),
  ('bokheim:industry:policy', 'unified:6cd36c31-3f37-5ae2-af25-b27360c7b523', 0, 120);

INSERT INTO source_selector (concept_id, system_id, selector, ordinal) VALUES
  ('bokheim:industry:general', 'lcc', 'HD2321..HD2328.9@1', 0),
  ('bokheim:industry:general', 'lcc', 'HD2340..HD2340.79@1', 1),
  ('bokheim:industry:general', 'lcc', 'HD2346.6..HD2350.79@1', 2),
  ('bokheim:industry:general', 'lcc', 'HD2356.1..HD2364.9@1', 3),
  ('bokheim:industry:general', 'lcc', 'HD2385.1..HD2420.9@1', 4),
  ('bokheim:industry:general', 'lcc', 'HD2429.1..HD2708.9@1', 5),
  ('bokheim:industry:general', 'lcc', 'HD2930.8..HD2950.9@1', 6),
  ('bokheim:industry:general', 'lcc', 'HD3575.1..HD3610.9@1', 7),
  ('bokheim:industry:industrialization', 'lcc', 'HD2329', 0),
  ('bokheim:industry:rural', 'lcc', 'HD2330', 0),
  ('unified:f7738f96-847e-5fa8-b686-8b1ffd53cb85', 'lcc', 'HD2331..HD2336.9@1', 0),
  ('bokheim:industry:sweatshops', 'lcc', 'HD2337..HD2339.9@1', 0),
  ('unified:087cc6ae-5357-5c3c-a220-58af778dd88a', 'lcc', 'HD2340.8..HD2346.5@1', 0),
  ('bokheim:industry:large', 'lcc', 'HD2350.8..HD2356@1', 0),
  ('bokheim:industry:contracting', 'lcc', 'HD2365..HD2385@1', 0),
  ('bokheim:industry:trade-associations', 'lcc', 'HD2421..HD2429@1', 0),
  ('bokheim:industry:corporations', 'lcc', 'HD2709..HD2930.7@1', 0),
  ('bokheim:industry:cooperatives', 'lcc', 'HD2951..HD3575@1', 0),
  ('bokheim:industry:policy', 'lcc', 'HD3611..HD4730.9@1', 0);

COMMIT;

-- Partition TS1300-TS1865 among mutually exclusive leaves. Only the two
-- uncovered subdivisions above the observed-use threshold become new topics.

INSERT INTO concept(concept_id, preferred_label) VALUES
  (53388, 'Textile Industry Reference & Practice'),
  (53389, 'Modern Textile Industry'),
  (53390, 'Textile Industry Special Studies');

INSERT INTO concept_parent(concept_id, parent_concept_id, ordinal) VALUES
  (53388, 358, 0),
  (53389, 358, 1),
  (53390, 358, 2);

DELETE FROM lcc_range
WHERE concept_id IN (
  358, 6223, 6426, 6505, 6733, 6863, 6994, 7347, 7867, 9119,
  9453, 9605, 9905
);

-- Broad residual portions of the pruned schedule.
INSERT OR REPLACE INTO lcc_range(start_code, end_code, concept_id) VALUES
  ('TS1300',   'TS1314.99', 53388),
  ('TS1318.1', 'TS1320.99', 53388),
  ('TS1424.51','TS1438.99', 53388),
  ('TS1440.1', 'TS1444.99', 53388),
  ('TS1446.1', 'TS1448.99', 53388),
  ('TS1450',   'TS1474.49', 53388),
  ('TS1476',   'TS1479.99', 53388),
  ('TS1488',   'TS1489.99', 53388),
  ('TS1500.1', 'TS1509.99', 53388),
  ('TS1520.1', 'TS1524.99', 53388),
  ('TS1526',   'TS1539.99', 53388),
  ('TS1590.1', 'TS1599.99', 53388),
  ('TS1635.1', 'TS1639.99', 53388),
  ('TS1688.1', 'TS1699.99', 53388),
  ('TS1735.1', 'TS1759.99', 53388),
  ('TS1770.21','TS1771.99', 53388),
  ('TS1779.51','TS1782.99', 53388),
  ('TS1850.1', 'TS1865',    53388);

INSERT OR REPLACE INTO lcc_range(start_code, end_code, concept_id) VALUES
  ('TS1445', 'TS1445.99', 53389),
  ('TS1449', 'TS1449.99', 53390),
  ('TS1315', 'TS1318',    6863),
  ('TS1321', 'TS1424.5',  6426),
  ('TS1439', 'TS1440',    7347),
  ('TS1446', 'TS1446',    8063),
  ('TS1474.5','TS1474.99',9539),
  ('TS1475', 'TS1475.99', 6907),
  ('TS1480', 'TS1487.99', 9605),
  ('TS1490', 'TS1500',    6994),
  ('TS1510', 'TS1511.99', 8333),
  ('TS1512', 'TS1512.99', 9604),
  ('TS1513', 'TS1520',    8333),
  ('TS1525', 'TS1525.99', 8509),
  ('TS1540', 'TS1549',    8982),
  ('TS1550', 'TS1590',    6505),
  ('TS1600', 'TS1635',    9119),
  ('TS1640', 'TS1688',    7867),
  ('TS1700', 'TS1735',    6223),
  ('TS1760', 'TS1770.2',  9905),
  ('TS1772', 'TS1779.5',  6733),
  ('TS1783', 'TS1783.99', 9893),
  ('TS1784', 'TS1850',    9453);

INSERT OR IGNORE INTO lcc_selector(concept_id, selector)
VALUES (358, 'TS1300-1865');

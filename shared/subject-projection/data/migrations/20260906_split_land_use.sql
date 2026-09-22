BEGIN IMMEDIATE;

DELETE FROM source_selector
WHERE concept_id IN (
  'unified:abc1f0a6-b2c0-586e-8714-dbe9042848fa',
  'bokheim:land-use:general-history',
  'bokheim:land-use:by-region',
  'bokheim:land-use:tenure',
  'bokheim:land-use:communal-public-ownership',
  'bokheim:land-use:large-holdings',
  'bokheim:land-use:reform-consolidation',
  'bokheim:land-use:smallholdings',
  'bokheim:land-use:real-estate-business'
);

INSERT OR IGNORE INTO concept (concept_id, preferred_label) VALUES
  ('bokheim:land-use:general-history', 'Land Use: General & History'),
  ('bokheim:land-use:by-region', 'Land Use by Region'),
  ('bokheim:land-use:tenure', 'Land Tenure'),
  ('bokheim:land-use:communal-public-ownership', 'Communal & Public Land Ownership'),
  ('bokheim:land-use:large-holdings', 'Large Estates & Landlord Systems'),
  ('bokheim:land-use:reform-consolidation', 'Land Reform & Consolidation'),
  ('bokheim:land-use:smallholdings', 'Smallholdings & Peasant Proprietorship'),
  ('bokheim:land-use:real-estate-business', 'Real-Estate Business');

INSERT OR IGNORE INTO concept_parent
  (concept_id, parent_concept_id, ordinal, display_order) VALUES
  ('bokheim:land-use:general-history', 'unified:abc1f0a6-b2c0-586e-8714-dbe9042848fa', 0, 10),
  ('bokheim:land-use:by-region', 'unified:abc1f0a6-b2c0-586e-8714-dbe9042848fa', 0, 20),
  ('bokheim:land-use:tenure', 'unified:abc1f0a6-b2c0-586e-8714-dbe9042848fa', 0, 30),
  ('bokheim:land-use:communal-public-ownership', 'unified:abc1f0a6-b2c0-586e-8714-dbe9042848fa', 0, 40),
  ('bokheim:land-use:large-holdings', 'unified:abc1f0a6-b2c0-586e-8714-dbe9042848fa', 0, 50),
  ('bokheim:land-use:reform-consolidation', 'unified:abc1f0a6-b2c0-586e-8714-dbe9042848fa', 0, 60),
  ('bokheim:land-use:smallholdings', 'unified:abc1f0a6-b2c0-586e-8714-dbe9042848fa', 0, 70),
  ('bokheim:land-use:real-estate-business', 'unified:abc1f0a6-b2c0-586e-8714-dbe9042848fa', 0, 80);

-- These curated leaf ranges partition HD101-1395.5 and deliberately absorb
-- the small unassigned numeric gaps between official headline divisions.
INSERT INTO source_selector (concept_id, system_id, selector, ordinal) VALUES
  ('bokheim:land-use:general-history', 'lcc', 'HD101..HD165.9@1', 0),
  ('bokheim:land-use:by-region', 'lcc', 'HD166..HD1138.9@1', 0),
  ('bokheim:land-use:tenure', 'lcc', 'HD1139.2..HD1285.9@1', 0),
  ('bokheim:land-use:communal-public-ownership', 'lcc', 'HD1286..HD1325.9@1', 0),
  ('bokheim:land-use:large-holdings', 'lcc', 'HD1326..HD1331.9@1', 0),
  ('bokheim:land-use:reform-consolidation', 'lcc', 'HD1332..HD1335.9@1', 0),
  ('bokheim:land-use:smallholdings', 'lcc', 'HD1336..HD1360.9@1', 0),
  ('bokheim:land-use:real-estate-business', 'lcc', 'HD1361..HD1395.5.Z@1', 0);

COMMIT;

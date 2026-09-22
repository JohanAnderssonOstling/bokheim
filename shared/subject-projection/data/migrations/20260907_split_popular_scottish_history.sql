BEGIN;

INSERT INTO concept(preferred_label) VALUES
  ('Scottish Political & Diplomatic History'),
  ('Scottish Antiquities'),
  ('Scottish Social & Cultural History'),
  ('Scottish Ethnography'),
  ('Scots Abroad'),
  ('Scottish Biographies'),
  ('Scottish Highlands & Clans'),
  ('Hebrides History'),
  ('Orkney Islands History'),
  ('Shetland Islands History'),
  ('Edinburgh History'),
  ('Glasgow History'),
  ('Aberdeen History');

INSERT INTO concept_parent(concept_id, parent_concept_id, ordinal)
SELECT concept_id, 3530, 0 FROM concept WHERE preferred_label IN (
  'Scottish Political & Diplomatic History', 'Scottish Antiquities',
  'Scottish Social & Cultural History', 'Scottish Ethnography',
  'Scots Abroad', 'Scottish Biographies', 'Scottish Highlands & Clans',
  'Hebrides History', 'Orkney Islands History', 'Shetland Islands History',
  'Edinburgh History', 'Glasgow History', 'Aberdeen History'
);

INSERT INTO lcc_range(class_letters, start_value, end_value, concept_id)
SELECT 'DA', '880.H6', '880.H76', concept_id FROM concept
WHERE preferred_label = 'Scottish Highlands & Clans';
INSERT INTO lcc_range(class_letters, start_value, end_value, concept_id)
SELECT 'DA', '880.O5', '880.O6', concept_id FROM concept
WHERE preferred_label = 'Orkney Islands History';
INSERT INTO lcc_range(class_letters, start_value, end_value, concept_id)
SELECT 'DA', '890.E2', '890.E4', concept_id FROM concept
WHERE preferred_label = 'Edinburgh History';
INSERT INTO lcc_range(class_letters, start_value, end_value, concept_id)
SELECT 'DA', '890.G49', '890.G5', concept_id FROM concept
WHERE preferred_label = 'Glasgow History';
INSERT INTO lcc_range(class_letters, start_value, end_value, concept_id)
SELECT 'DA', '890.E3A2', '890.E3Z', 2461;

INSERT INTO lcc_selector(concept_id, selector)
SELECT concept_id, 'DA765' FROM concept WHERE preferred_label = 'Scottish Political & Diplomatic History';
INSERT INTO lcc_selector(concept_id, selector)
SELECT concept_id, 'DA770' FROM concept WHERE preferred_label = 'Scottish Antiquities';
INSERT INTO lcc_selector(concept_id, selector)
SELECT concept_id, 'DA772' FROM concept WHERE preferred_label = 'Scottish Social & Cultural History';
INSERT INTO lcc_selector(concept_id, selector)
SELECT concept_id, 'DA774' FROM concept WHERE preferred_label = 'Scottish Ethnography';
INSERT INTO lcc_selector(concept_id, selector)
SELECT concept_id, 'DA774.5' FROM concept WHERE preferred_label = 'Scots Abroad';
INSERT INTO lcc_selector(concept_id, selector)
SELECT concept_id, 'DA760' FROM concept WHERE preferred_label = 'Scottish Biographies';
INSERT INTO lcc_selector(concept_id, selector)
SELECT concept_id, 'DA880.H4' FROM concept WHERE preferred_label = 'Hebrides History';
INSERT INTO lcc_selector(concept_id, selector)
SELECT concept_id, 'DA880.S5' FROM concept WHERE preferred_label = 'Shetland Islands History';
INSERT INTO lcc_selector(concept_id, selector)
SELECT concept_id, 'DA890.A2' FROM concept WHERE preferred_label = 'Aberdeen History';

INSERT OR IGNORE INTO lcc_selector(concept_id, selector) VALUES (2461, 'DA870');

COMMIT;

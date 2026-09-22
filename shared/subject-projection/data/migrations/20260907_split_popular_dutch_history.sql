BEGIN;

INSERT INTO concept(preferred_label) VALUES
  ('Dutch History in the 19th Century'),
  ('Wilhelmina, 1890–1948'),
  ('Juliana, 1948–1980'),
  ('Beatrix, 1980–2013'),
  ('Willem-Alexander, 2013–Present'),
  ('Dutch Antiquities'),
  ('Dutch Social & Cultural History'),
  ('Dutch Ethnography'),
  ('Dutch Culture Abroad'),
  ('Amsterdam History'),
  ('Rotterdam History'),
  ('Utrecht History');

INSERT INTO concept_parent(concept_id, parent_concept_id, ordinal)
SELECT concept_id, 11531, 0 FROM concept WHERE preferred_label IN (
  'Dutch History in the 19th Century', 'Wilhelmina, 1890–1948',
  'Juliana, 1948–1980', 'Beatrix, 1980–2013',
  'Willem-Alexander, 2013–Present'
);
INSERT INTO concept_parent(concept_id, parent_concept_id, ordinal)
SELECT concept_id, 3579, 0 FROM concept WHERE preferred_label IN (
  'Dutch Antiquities', 'Dutch Social & Cultural History', 'Dutch Ethnography',
  'Dutch Culture Abroad', 'Amsterdam History', 'Rotterdam History', 'Utrecht History'
);

INSERT INTO lcc_range(class_letters, start_value, end_value, concept_id)
SELECT 'DJ', '215', '263.Z', concept_id FROM concept
WHERE preferred_label = 'Dutch History in the 19th Century';
INSERT INTO lcc_range(class_letters, start_value, end_value, concept_id)
SELECT 'DJ', '281', '287.Z', concept_id FROM concept
WHERE preferred_label = 'Wilhelmina, 1890–1948';
INSERT INTO lcc_range(class_letters, start_value, end_value, concept_id)
SELECT 'DJ', '288', '289.Z', concept_id FROM concept
WHERE preferred_label = 'Juliana, 1948–1980';
INSERT INTO lcc_range(class_letters, start_value, end_value, concept_id)
SELECT 'DJ', '290', '292.Z', concept_id FROM concept
WHERE preferred_label = 'Beatrix, 1980–2013';
INSERT INTO lcc_range(class_letters, start_value, end_value, concept_id)
SELECT 'DJ', '293', '295.Z', concept_id FROM concept
WHERE preferred_label = 'Willem-Alexander, 2013–Present';
INSERT INTO lcc_range(class_letters, start_value, end_value, concept_id)
SELECT 'DJ', '91', '92.Z', concept_id FROM concept
WHERE preferred_label = 'Dutch Ethnography';
INSERT INTO lcc_range(class_letters, start_value, end_value, concept_id)
SELECT 'DJ', '411.A5', '411.A59', concept_id FROM concept
WHERE preferred_label = 'Amsterdam History';

INSERT INTO lcc_selector(concept_id, selector)
SELECT concept_id, 'DJ51' FROM concept WHERE preferred_label = 'Dutch Antiquities';
INSERT INTO lcc_selector(concept_id, selector)
SELECT concept_id, 'DJ71' FROM concept WHERE preferred_label = 'Dutch Social & Cultural History';
INSERT INTO lcc_selector(concept_id, selector)
SELECT concept_id, 'DJ81' FROM concept WHERE preferred_label = 'Dutch Culture Abroad';
INSERT INTO lcc_selector(concept_id, selector)
SELECT concept_id, 'DJ411.R8' FROM concept WHERE preferred_label = 'Rotterdam History';
INSERT INTO lcc_selector(concept_id, selector)
SELECT concept_id, 'DJ411.U9' FROM concept WHERE preferred_label = 'Utrecht History';

INSERT OR IGNORE INTO lcc_selector(concept_id, selector) VALUES (2455, 'DJ16');

COMMIT;

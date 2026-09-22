BEGIN;

INSERT INTO concept(preferred_label) VALUES
  ('Romanian Social & Cultural History'),
  ('Romanian Ethnography'),
  ('Romanian Antiquities'),
  ('Romanian Political & Diplomatic History'),
  ('Romanian Biographies'),
  ('Bucharest History'),
  ('Banat History'),
  ('Dobruja History'),
  ('Cluj-Napoca History'),
  ('Timișoara History');

INSERT INTO concept_parent(concept_id, parent_concept_id, ordinal)
SELECT concept_id, 3548, 0 FROM concept WHERE preferred_label IN (
  'Romanian Social & Cultural History', 'Romanian Ethnography',
  'Romanian Antiquities', 'Romanian Political & Diplomatic History',
  'Romanian Biographies', 'Bucharest History', 'Banat History',
  'Dobruja History', 'Cluj-Napoca History', 'Timișoara History'
);

INSERT INTO lcc_range(class_letters, start_value, end_value, concept_id)
SELECT 'DR', '213', '214.Z', concept_id FROM concept
WHERE preferred_label = 'Romanian Ethnography';
INSERT INTO lcc_range(class_letters, start_value, end_value, concept_id)
SELECT 'DR', '226', '229.Z', concept_id FROM concept
WHERE preferred_label = 'Romanian Political & Diplomatic History';
INSERT INTO lcc_range(class_letters, start_value, end_value, concept_id)
SELECT 'DR', '216', '216.6', concept_id FROM concept
WHERE preferred_label = 'Romanian Biographies';
INSERT INTO lcc_range(class_letters, start_value, end_value, concept_id)
SELECT 'DR', '286', '286.8.Z', concept_id FROM concept
WHERE preferred_label = 'Bucharest History';

INSERT INTO lcc_selector(concept_id, selector)
SELECT concept_id, 'DR212' FROM concept WHERE preferred_label = 'Romanian Social & Cultural History';
INSERT INTO lcc_selector(concept_id, selector)
SELECT concept_id, 'DR211' FROM concept WHERE preferred_label = 'Romanian Antiquities';
INSERT INTO lcc_selector(concept_id, selector)
SELECT concept_id, 'DR281.B25' FROM concept WHERE preferred_label = 'Banat History';
INSERT INTO lcc_selector(concept_id, selector)
SELECT concept_id, 'DR281.D5' FROM concept WHERE preferred_label = 'Dobruja History';
INSERT INTO lcc_selector(concept_id, selector)
SELECT concept_id, 'DR296.C55' FROM concept WHERE preferred_label = 'Cluj-Napoca History';
INSERT INTO lcc_selector(concept_id, selector)
SELECT concept_id, 'DR296.T5' FROM concept WHERE preferred_label = 'Timișoara History';

INSERT OR IGNORE INTO lcc_selector(concept_id, selector) VALUES
  (13893, 'DR204.5'),
  (13893, 'DR279.3');

COMMIT;

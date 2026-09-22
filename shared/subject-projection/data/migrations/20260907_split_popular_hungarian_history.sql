BEGIN;

INSERT INTO concept(preferred_label) VALUES
  ('Hungarian History in the 20th Century'),
  ('Hungary, 1918–1945'),
  ('20th-Century Hungarian Social & Cultural History'),
  ('Hungarian Political & Diplomatic History'),
  ('Early Hungary, to 896'),
  ('Budapest History');

INSERT INTO concept_parent(concept_id, parent_concept_id, ordinal)
SELECT concept_id, 3539, 0 FROM concept WHERE preferred_label IN (
  'Hungarian History in the 20th Century',
  'Hungarian Political & Diplomatic History',
  'Early Hungary, to 896',
  'Budapest History'
);
INSERT INTO concept_parent(concept_id, parent_concept_id, ordinal)
SELECT concept_id,
       (SELECT concept_id FROM concept WHERE preferred_label = 'Hungarian History in the 20th Century'),
       0
FROM concept WHERE preferred_label = 'Hungary, 1918–1945';
INSERT INTO concept_parent(concept_id, parent_concept_id, ordinal)
SELECT concept_id,
       (SELECT concept_id FROM concept WHERE preferred_label = 'Hungarian History in the 20th Century'),
       0
FROM concept WHERE preferred_label = '20th-Century Hungarian Social & Cultural History';

-- Keep World War II reachable through both the national and chronological paths.
INSERT INTO concept_parent(concept_id, parent_concept_id, ordinal)
SELECT 10512, concept_id, 0 FROM concept WHERE preferred_label = 'Hungary, 1918–1945';

UPDATE lcc_range SET concept_id = (SELECT concept_id FROM concept
  WHERE preferred_label = 'Hungarian History in the 20th Century')
WHERE class_letters = 'DB' AND start_value = '947' AND end_value = '958.6';
UPDATE lcc_range SET concept_id = (SELECT concept_id FROM concept
  WHERE preferred_label = 'Hungary, 1918–1945')
WHERE class_letters = 'DB' AND start_value = '954' AND end_value = '955.8';
UPDATE lcc_range SET concept_id = (SELECT concept_id FROM concept
  WHERE preferred_label = 'Hungarian Political & Diplomatic History')
WHERE class_letters = 'DB' AND start_value = '926' AND end_value = '926.3';
UPDATE lcc_range SET concept_id = (SELECT concept_id FROM concept
  WHERE preferred_label = 'Early Hungary, to 896')
WHERE class_letters = 'DB' AND start_value = '927' AND end_value = '928';
UPDATE lcc_range SET concept_id = (SELECT concept_id FROM concept
  WHERE preferred_label = 'Budapest History')
WHERE class_letters = 'DB' AND start_value = '981' AND end_value = '997';

INSERT INTO lcc_selector(concept_id, selector)
SELECT concept_id, 'DB949.2' FROM concept
WHERE preferred_label = '20th-Century Hungarian Social & Cultural History';

INSERT OR IGNORE INTO lcc_selector(concept_id, selector) VALUES
  (13996, 'DB905'),
  (13996, 'DB983.5');

COMMIT;

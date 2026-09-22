BEGIN;

INSERT INTO concept(preferred_label) VALUES
  ('Austrian History in the 20th Century'),
  ('Franz Joseph I, 1848–1916'),
  ('Empress Elisabeth of Austria'),
  ('Archduke Franz Ferdinand'),
  ('Crown Prince Rudolf'),
  ('Austrian Republic since 1918');

INSERT INTO concept_parent(concept_id, parent_concept_id, ordinal)
SELECT concept_id, 3537, 0 FROM concept
WHERE preferred_label IN (
  'Austrian History in the 20th Century',
  'Franz Joseph I, 1848–1916',
  'Empress Elisabeth of Austria',
  'Archduke Franz Ferdinand',
  'Crown Prince Rudolf',
  'Austrian Republic since 1918'
);

-- DB84–DB98.9 was incorrectly collapsed into the nineteenth century.
UPDATE lcc_range SET end_value = '90.Z'
WHERE class_letters = 'DB' AND start_value = '84' AND end_value = '98.9'
  AND concept_id = 11558;

INSERT INTO lcc_range(class_letters, start_value, end_value, concept_id)
SELECT 'DB', '91', '98.9', concept_id FROM concept
WHERE preferred_label = 'Austrian History in the 20th Century';
INSERT INTO lcc_range(class_letters, start_value, end_value, concept_id)
SELECT 'DB', '85', '90.Z', concept_id FROM concept
WHERE preferred_label = 'Franz Joseph I, 1848–1916';
INSERT INTO lcc_range(class_letters, start_value, end_value, concept_id)
SELECT 'DB', '96', '98.9', concept_id FROM concept
WHERE preferred_label = 'Austrian Republic since 1918';

INSERT INTO lcc_selector(concept_id, selector)
SELECT concept_id, 'DB88' FROM concept WHERE preferred_label = 'Empress Elisabeth of Austria';
INSERT INTO lcc_selector(concept_id, selector)
SELECT concept_id, 'DB89.F7' FROM concept WHERE preferred_label = 'Archduke Franz Ferdinand';
INSERT INTO lcc_selector(concept_id, selector)
SELECT concept_id, 'DB89.R8' FROM concept WHERE preferred_label = 'Crown Prince Rudolf';

INSERT OR IGNORE INTO lcc_selector(concept_id, selector) VALUES
  (2456, 'DB16'),
  (2456, 'DB849');

COMMIT;

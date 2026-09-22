BEGIN;

-- Keep the broad modern chronology navigable while allowing the two large
-- official LCC period blocks to resolve more specifically.
INSERT INTO concept(concept_id, preferred_label) VALUES
  (51546, 'Greek History, 1821–1913'),
  (51547, 'Greek History since 1913'),
  (51548, 'Greek Civil War, 1944–1949'),
  (51549, 'Restoration of Democracy, 1974–Present');

INSERT INTO concept_parent(concept_id, parent_concept_id, ordinal) VALUES
  (51546, 3568, 4),
  (51547, 3568, 5),
  (51548, 51547, 0),
  (51549, 51547, 1);

UPDATE lcc_range SET concept_id = 51546
WHERE class_letters = 'DF' AND start_value = '803' AND end_value = '832';
UPDATE lcc_range SET concept_id = 51547
WHERE class_letters = 'DF' AND start_value = '833' AND end_value = '854.32';

INSERT INTO lcc_range(class_letters, start_value, end_value, concept_id) VALUES
  ('DF', '849.5', '849.58', 51548),
  ('DF', '854',   '854.32', 51549);

-- DF716 is explicitly a guidebook number, not history proper.
INSERT OR IGNORE INTO lcc_selector(concept_id, selector) VALUES (2462, 'DF716');

COMMIT;

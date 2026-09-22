-- Partition TP785-TP869 around the established pottery, kiln, and glass leaves.

INSERT INTO concept(concept_id, preferred_label)
VALUES (53394, 'Ceramics Industry Reference & Materials');

INSERT INTO concept_parent(concept_id, parent_concept_id, ordinal)
VALUES (53394, 217, 0);

DELETE FROM lcc_range
WHERE concept_id = 217;

INSERT INTO lcc_range(start_code, end_code, concept_id) VALUES
  ('TP785',   'TP806.99', 53394),
  ('TP823.1', 'TP840.99', 53394),
  ('TP842.1', 'TP844.99', 53394);

INSERT OR IGNORE INTO lcc_selector(concept_id, selector) VALUES
  (217, 'TP785-869'),
  (217, 'TP791-794'),
  (217, 'TP797-804'),
  (217, 'TP805-806'),
  (217, 'TP825-842');

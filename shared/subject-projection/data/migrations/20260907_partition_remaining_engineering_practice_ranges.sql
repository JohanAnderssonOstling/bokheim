-- Partition the remaining high-use TA practice ranges around their established
-- children. Printed broad intervals remain exact selectors on nonleaf parents.

INSERT INTO concept(concept_id, preferred_label) VALUES
  (53383, 'Ergonomics Reference & Methods'),
  (53384, 'Environmental Engineering Reference & Methods'),
  (53385, 'Engineering Economy Reference & Practice');

INSERT INTO concept_parent(concept_id, parent_concept_id, ordinal) VALUES
  (53383, 287, 0),
  (53384, 286, 0),
  (53385, 281, 0);

DELETE FROM lcc_range
WHERE (concept_id = 287 AND start_code = 'TA166' AND end_code = 'TA167')
   OR (concept_id = 286 AND start_code = 'TA170' AND end_code = 'TA171')
   OR (concept_id = 281 AND start_code = 'TA177.4' AND end_code = 'TA185');

INSERT INTO lcc_range(start_code, end_code, concept_id) VALUES
  ('TA166',   'TA166.99', 53383),
  ('TA170',   'TA170.99', 53384),
  ('TA177.4', 'TA177.69', 53385),
  ('TA177.71','TA179.99', 53385),
  ('TA182',   'TA182.99', 53385),
  ('TA184',   'TA185',    53385);

INSERT OR IGNORE INTO lcc_selector(concept_id, selector) VALUES
  (287, 'TA166-167'),
  (286, 'TA170-171'),
  (281, 'TA177.4-185');

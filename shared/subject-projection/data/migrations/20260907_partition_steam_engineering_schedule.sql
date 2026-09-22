-- Partition the steam-education and locomotive schedules around established
-- children without creating a low-value "General works" period leaf.

INSERT INTO concept(concept_id, preferred_label) VALUES
  (53395, 'Steam Engineering Reference & Education'),
  (53396, 'Locomotive Engineering Reference & Operations');

INSERT INTO concept_parent(concept_id, parent_concept_id, ordinal) VALUES
  (53395, 389, 0),
  (53396, 389, 1);

DELETE FROM lcc_range
WHERE concept_id = 389;

INSERT INTO lcc_range(start_code, end_code, concept_id) VALUES
  ('TJ280',    'TJ280.5',  53395),
  ('TJ603.41', 'TJ603.48', 53396),
  ('TJ603.51', 'TJ608.99', 53396),
  ('TJ623.1',  'TJ629.99', 53396),
  ('TJ631',    'TJ679.99', 53396),
  ('TJ683.1',  'TJ694.99', 53396),
  ('TJ630',    'TJ630.99', 8859),
  ('TJ695',    'TJ695.99', 9068);

INSERT OR IGNORE INTO lcc_selector(concept_id, selector) VALUES
  (389, 'TJ280-280.5'),
  (389, 'TJ603-695'),
  (389, 'TJ604-605'),
  (389, 'TJ640-656'),
  (389, 'TJ685-690');

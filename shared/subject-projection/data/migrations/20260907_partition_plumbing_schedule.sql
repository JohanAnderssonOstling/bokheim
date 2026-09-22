-- Partition TH6101-TH6887 into mutually exclusive leaves. Only TH6124 has
-- enough observed use to justify a new specific subject; the remaining pruned
-- schedule stays in a broad leaf.

INSERT INTO concept(concept_id, preferred_label) VALUES
  (53386, 'Plumbing Reference & Practice'),
  (53387, 'Popular & Juvenile Plumbing');

INSERT INTO concept_parent(concept_id, parent_concept_id, ordinal) VALUES
  (53386, 208, 0),
  (53387, 208, 1);

DELETE FROM lcc_range
WHERE concept_id = 208;

INSERT INTO lcc_range(start_code, end_code, concept_id) VALUES
  ('TH6101', 'TH6123.99', 53386),
  ('TH6124', 'TH6124.99', 53387),
  ('TH6125', 'TH6484.99', 53386),
  ('TH6512.1', 'TH6887', 53386);

INSERT OR IGNORE INTO lcc_selector(concept_id, selector)
VALUES (208, 'TH6101-6887');

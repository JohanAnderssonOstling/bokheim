BEGIN;

INSERT OR IGNORE INTO concept(concept_id, preferred_label) VALUES
  (51590, 'Ukrainian Independence, 1917–1921'),
  (51591, 'Soviet Ukraine, 1921–1944'),
  (51592, 'Holodomor, 1932–1933'),
  (51593, 'Soviet Ukraine, 1945–1991'),
  (51594, 'Post-Soviet Ukrainian Political History'),
  (51595, 'Post-Soviet Ukrainian Foreign Relations');

INSERT OR IGNORE INTO concept_parent(concept_id, parent_concept_id, ordinal) VALUES
  (51590, 11381, 0),
  (51591, 11381, 0),
  (51592, 51591, 0),
  (51593, 11381, 0),
  (51594, 13961, 0),
  (51595, 13961, 0);

INSERT OR REPLACE INTO lcc_range(class_letters, start_value, end_value, concept_id) VALUES
  ('DK', '508.832', '508.832.Z', 51590),
  ('DK', '508.833', '508.8378.Z', 51591),
  ('DK', '508.837', '508.8378.Z', 51592),
  ('DK', '508.84',  '508.843.Z', 51593),
  ('DK', '508.845', '508.851.Z', 13961),
  ('DK', '508.852', '508.852.Z', 13962),
  ('DK', '508.A5',  '508.Z',     13972);

INSERT OR IGNORE INTO lcc_selector(concept_id, selector) VALUES
  (51594, 'DK508.848'),
  (51595, 'DK508.849'),
  (13972, 'DK508.924');

COMMIT;

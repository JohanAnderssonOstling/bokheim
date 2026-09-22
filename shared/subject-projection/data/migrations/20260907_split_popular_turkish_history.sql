BEGIN;

INSERT INTO concept(concept_id, preferred_label) VALUES
  (51569, 'Mehmed V, 1909–1918'),
  (51570, 'Mehmed VI, 1918–1922'),
  (51571, 'First Turkish Republic & Kemalism, 1923–1960'),
  (51572, 'Mustafa Kemal Atatürk'),
  (51573, 'Second Turkish Republic, 1960–1980'),
  (51574, 'Third Turkish Republic, 1980–Present');

INSERT INTO concept_parent(concept_id, parent_concept_id, ordinal) VALUES
  (51569, 11401, 0),
  (51570, 11401, 0),
  (51571, 11401, 0),
  (51572, 51571, 0),
  (51573, 11401, 0),
  (51574, 11401, 0);

INSERT INTO lcc_range(class_letters, start_value, end_value, concept_id) VALUES
  ('DR', '583', '588',   51569),
  ('DR', '589', '589.Z', 51570),
  ('DR', '590', '592.Z', 51571),
  ('DR', '593', '601.Z', 51573),
  ('DR', '603', '605.Z', 51574);

INSERT INTO lcc_selector(concept_id, selector) VALUES
  (51572, 'DR592.K4'),
  (2488,  'DR416'),
  (2488,  'DR718');

COMMIT;

BEGIN;

INSERT INTO concept(concept_id, preferred_label) VALUES
  (51581, 'Northern Irish History'),
  (51582, 'Dublin History'),
  (51583, 'Belfast History'),
  (51584, 'Cork History'),
  (51585, 'Limerick History'),
  (51586, 'Derry History'),
  (51587, 'Irish Antiquities'),
  (51588, 'Irish Social & Cultural History'),
  (51589, 'Irish Ethnography');

INSERT INTO concept_parent(concept_id, parent_concept_id, ordinal) VALUES
  (51581, 3606, 0),
  (51582, 3606, 0),
  (51583, 3606, 0),
  (51584, 3606, 0),
  (51585, 3606, 0),
  (51586, 3606, 0),
  (51587, 3606, 0),
  (51588, 3606, 0),
  (51589, 3606, 0);

-- Repair two ranges written with IDs concurrently claimed by another migration.
DELETE FROM lcc_range
WHERE class_letters = 'DA'
  AND ((start_value = '990.U45' AND end_value = '990.U46')
    OR (start_value = '995.D75' AND end_value = '995.D9'));

INSERT INTO lcc_range(class_letters, start_value, end_value, concept_id) VALUES
  ('DA', '990.U45', '990.U46', 51581),
  ('DA', '995.D75', '995.D9',  51582);

INSERT INTO lcc_selector(concept_id, selector) VALUES
  (51583, 'DA995.B5'),
  (51584, 'DA995.C7'),
  (51585, 'DA995.L7'),
  (51586, 'DA995.L75'),
  (51587, 'DA920'),
  (51588, 'DA925'),
  (51589, 'DA927');

INSERT OR IGNORE INTO lcc_selector(concept_id, selector) VALUES (2463, 'DA980');

COMMIT;

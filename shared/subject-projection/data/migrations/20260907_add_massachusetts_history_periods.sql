BEGIN;

INSERT INTO concept(concept_id,preferred_label) VALUES
  (50979,'Early Massachusetts to 1775'),
  (50980,'1775–1865'),
  (50981,'1865–1950'),
  (50982,'1951–');

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal) VALUES
  (50979,3450,0),(50980,3450,0),(50981,3450,0),(50982,3450,0);

-- Remove the redundant 1951+ biography subranges before assigning the full era.
DELETE FROM lcc_range
WHERE concept_id=3450 AND class_letters='F'
  AND CAST(start_value AS REAL)>=71 AND CAST(end_value AS REAL)<72;

INSERT INTO lcc_range(class_letters,start_value,end_value,concept_id) VALUES
  ('F','67','68.999',50979),
  ('F','69','69.999',50980),
  ('F','70','70.999',50981),
  ('F','71','71.999',50982);

COMMIT;

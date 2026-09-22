-- Replace the overlapping GE1-GE350 umbrella with the official occupied
-- top-level divisions. Existing specialist children already own GE70-GE90,
-- GE170-GE190, GE195-GE199, and GE300-GE350.

INSERT INTO concept(concept_id,preferred_label)
VALUES
  (53322,'Environmental Science Reference & Methods'),
  (53323,'Environmental Science Museums & Public Works'),
  (53324,'Environmental Conditions & Quality'),
  (53325,'Environmental Justice');

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal)
VALUES
  (53322,2377,0),
  (53323,2377,0),
  (53324,2377,0),
  (53325,2377,0);

DELETE FROM lcc_range
WHERE concept_id=2377
  AND start_code='GE1'
  AND end_code='GE350';

INSERT INTO lcc_range(start_code,end_code,concept_id)
VALUES
  ('GE1','GE69',53322),
  ('GE95','GE139',53323),
  ('GE140','GE160',53324),
  ('GE220','GE240',53325);

-- Complete the official top-level QE partition.  Stratigraphy is already
-- covered by exact general headings plus chronological leaf ranges.

INSERT INTO concept(concept_id,preferred_label)
VALUES
  (53320,'Geology Reference & Institutions'),
  (53321,'Regional Geology');

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal)
VALUES
  (53320,4904,0),
  (53321,4904,0);

DELETE FROM lcc_range
WHERE concept_id=4904
  AND start_code='QE1'
  AND end_code='QE996.5';

INSERT INTO lcc_range(start_code,end_code,concept_id)
VALUES
  ('QE1','QE64',53320),
  ('QE65','QE350.62',53321);

-- Wholly contained by broader ranges on the same destination.
DELETE FROM lcc_range
WHERE (concept_id,start_code,end_code) IN (
  (4906,'QE363','QE363.2'),
  (4906,'QE366.15','QE366.16'),
  (4910,'QE431','QE431.2'),
  (4910,'QE432.7','QE433'),
  (4908,'QE704','QE704.3'),
  (4908,'QE711','QE711.2'),
  (4907,'QE904.5','QE904.52')
);

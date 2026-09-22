-- Complete the QA299.6-QA433 Analysis partition. Its existing specialist
-- children cover every occupied later division; only the opening reference
-- material and Differential Equations need structural ranges.

INSERT INTO concept(concept_id,preferred_label)
VALUES
  (53356,'Analysis Reference & Foundations'),
  (53357,'Differential Equations Reference & Study');

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal)
VALUES
  (53356,4922,0),
  (53357,4930,0);

DELETE FROM lcc_range WHERE concept_id=4922;

INSERT INTO lcc_range(start_code,end_code,concept_id)
VALUES
  ('QA299.6','QA299.79',53356),
  ('QA299.83','QA302.Z',53356),
  ('QA370','QA371.Z',53357),
  ('QA372','QA373.Z',4931),
  ('QA374','QA380',4932);

INSERT OR IGNORE INTO lcc_selector(concept_id,selector)
VALUES
  (4922,'QA299.6-433'),
  (4930,'QA370-380');

-- Wholly contained by broader ranges on the same destination.
DELETE FROM lcc_range
WHERE (concept_id,start_code,end_code) IN (
  (4926,'QA312','QA312.5'),
  (4926,'QA315','QA316'),
  (4937,'QA329','QA329.9')
);

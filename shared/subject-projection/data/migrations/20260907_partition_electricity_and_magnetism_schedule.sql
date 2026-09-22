-- Fully partition QC501-QC766. Broad printed divisions remain exact on their
-- nonleaf nodes; ordinary numbered calls are owned by leaves.

INSERT INTO concept(concept_id,preferred_label)
VALUES (53348,'Electricity Reference & Textbooks');

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal)
VALUES (53348,6075,0);

INSERT OR IGNORE INTO lcc_selector(concept_id,selector)
SELECT 53348,selector FROM lcc_selector WHERE concept_id=6075;
DELETE FROM lcc_selector WHERE concept_id=6075;

INSERT INTO lcc_range(start_code,end_code,concept_id)
VALUES
  ('QC501','QC513.Z',53348),
  ('QC518','QC529.Z',53348);

DELETE FROM lcc_range WHERE concept_id IN (5034,6073,6074);

INSERT OR IGNORE INTO lcc_selector(concept_id,selector)
VALUES
  (5034,'QC501-766'),
  (6073,'QC501-721'),
  (6074,'QC750-766');

-- Complete the official opening Magnetism leaves.
DELETE FROM lcc_range
WHERE concept_id IN (6080,6081);

INSERT INTO lcc_range(start_code,end_code,concept_id)
VALUES
  ('QC750','QC755.4',6080),
  ('QC755.6','QC755.8',6081);

-- Wholly contained by the broader range on the same destination.
DELETE FROM lcc_range
WHERE concept_id=6096
  AND start_code='QC610.9'
  AND end_code='QC611.8';

-- Replace the restored QK umbrella with the complete official top-level Botany
-- partition.  Each newly exposed division has substantial observed use.

INSERT INTO concept(concept_id,preferred_label)
VALUES
  (53311,'Botany Reference & Institutions'),
  (53312,'Plant Geography');

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal)
VALUES
  (53311,4797,0),
  (53312,4797,0);

DELETE FROM lcc_range
WHERE concept_id=4797
  AND start_code='QK1'
  AND end_code='QK989';

INSERT INTO lcc_range(start_code,end_code,concept_id)
VALUES
  ('QK1','QK100',53311),
  ('QK101','QK474.5',53312),
  ('QK474.8','QK495.Z',4802);

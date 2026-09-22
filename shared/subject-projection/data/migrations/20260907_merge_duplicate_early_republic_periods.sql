BEGIN;

-- The broad period concepts already existed under Historical Eras. Make those
-- same concepts children of Early Republic as a second path, rather than
-- retaining duplicate period nodes.
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal) VALUES
  (3403,3417,1),
  (3411,3417,1),
  (3415,3417,1);

-- Move the new administration children to the established periods.
UPDATE concept_parent SET parent_concept_id=3403 WHERE parent_concept_id=51101;
UPDATE concept_parent SET parent_concept_id=3411 WHERE parent_concept_id=51105;
UPDATE concept_parent SET parent_concept_id=3415 WHERE parent_concept_id=51113;

-- Replace the established periods' former overlapping umbrella ranges with
-- the mutually exclusive residual ranges prepared by the split migration.
DELETE FROM lcc_range WHERE concept_id IN (3403,3411,3415);
UPDATE lcc_range SET concept_id=3403 WHERE concept_id=51101;
UPDATE lcc_range SET concept_id=3411 WHERE concept_id=51105;
UPDATE lcc_range SET concept_id=3415 WHERE concept_id=51113;

DELETE FROM concept WHERE concept_id IN (51101,51105,51113);

COMMIT;

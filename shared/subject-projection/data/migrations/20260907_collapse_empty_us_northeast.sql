BEGIN;

-- Northeast carried no selectors or ranges, so expose its two useful regions
-- directly beneath Regions of the U.S.
UPDATE concept_parent
SET parent_concept_id=3418
WHERE parent_concept_id=3401
  AND concept_id IN (3419,3421);

DELETE FROM concept_parent WHERE concept_id=3401;
DELETE FROM concept WHERE concept_id=3401;

COMMIT;

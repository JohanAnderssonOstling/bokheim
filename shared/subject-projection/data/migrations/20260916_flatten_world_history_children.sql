-- Make World the broad general-history node and flatten its thematic
-- children into the History root.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

-- Absorb the two general World concepts into World itself.
UPDATE concept_parent
SET parent_concept_id = 3701
WHERE parent_concept_id IN (10472,13686);
UPDATE lcc_range SET concept_id = 3701 WHERE concept_id IN (10472,13686);
INSERT OR IGNORE INTO lcc_selector(concept_id, selector)
SELECT 3701, selector FROM lcc_selector WHERE concept_id IN (10472,13686);
DELETE FROM lcc_selector WHERE concept_id IN (10472,13686);
DELETE FROM bisac_selector WHERE concept_id IN (10472,13686);
DELETE FROM source_label WHERE concept_id IN (10472,13686);
DELETE FROM concept_parent WHERE concept_id IN (10472,13686);
DELETE FROM concept WHERE concept_id IN (10472,13686);

-- The remaining World children are History-root thematic subjects.
UPDATE concept_parent
SET parent_concept_id = 3319
WHERE parent_concept_id = 3701;

COMMIT;

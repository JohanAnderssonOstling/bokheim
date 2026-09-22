-- Flatten National biography -> America; retain the broad source range on the parent.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

INSERT OR IGNORE INTO lcc_selector(concept_id,selector)
SELECT 3221,selector FROM lcc_selector WHERE concept_id=13349;

INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT cp.concept_id,3221,
       (SELECT COALESCE(MAX(ordinal)+1,0) FROM concept_parent WHERE parent_concept_id=3221)
       + ROW_NUMBER() OVER (ORDER BY cp.ordinal) - 1
FROM concept_parent cp
WHERE cp.parent_concept_id=13349;

DELETE FROM lcc_selector WHERE concept_id=13349;
DELETE FROM bisac_selector WHERE concept_id=13349;
DELETE FROM source_label WHERE concept_id=13349;
DELETE FROM concept_parent WHERE concept_id=13349;
DELETE FROM concept_parent WHERE parent_concept_id=13349;
DELETE FROM concept WHERE concept_id=13349;

COMMIT;

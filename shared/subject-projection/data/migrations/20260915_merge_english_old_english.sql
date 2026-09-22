-- Merge the redundant English & Old English wrapper into English.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

INSERT OR IGNORE INTO concept_parent (concept_id,parent_concept_id,ordinal)
SELECT cp.concept_id,5585,
       (SELECT COALESCE(MAX(ordinal),-1)+1 FROM concept_parent WHERE parent_concept_id=5585)
       + ROW_NUMBER() OVER (ORDER BY cp.ordinal,cp.concept_id)-1
FROM concept_parent cp
WHERE cp.parent_concept_id=3911
  AND cp.concept_id NOT IN (SELECT concept_id FROM concept_parent WHERE parent_concept_id=5585);

INSERT OR IGNORE INTO source_label (concept_id,label,ordinal)
SELECT 5585,label,
       (SELECT COALESCE(MAX(ordinal),-1)+1 FROM source_label WHERE concept_id=5585)
       + ROW_NUMBER() OVER (ORDER BY ordinal)-1
FROM source_label
WHERE concept_id=3911;

DELETE FROM concept_parent WHERE concept_id=3911 OR parent_concept_id=3911;
DELETE FROM lcc_selector WHERE concept_id=3911;
DELETE FROM concept WHERE concept_id=3911;

COMMIT;

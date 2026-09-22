-- Consolidate World period concepts into the thematic Eras hierarchy.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

-- Merge the LCC World 21st-century concept into the existing Modern-era
-- 21st Century concept, preserving its range and children.
UPDATE concept_parent SET parent_concept_id = 3674
WHERE parent_concept_id = 10669;
UPDATE lcc_range SET concept_id = 3674 WHERE concept_id = 10669;
INSERT OR IGNORE INTO lcc_selector(concept_id, selector)
SELECT 3674, selector FROM lcc_selector WHERE concept_id = 10669;
DELETE FROM lcc_selector WHERE concept_id = 10669;
DELETE FROM bisac_selector WHERE concept_id = 10669;
DELETE FROM source_label WHERE concept_id = 10669;
DELETE FROM concept_parent WHERE concept_id = 10669;
DELETE FROM concept WHERE concept_id = 10669;

-- The LCC transitional Medieval & Modern range belongs with the other eras.
UPDATE concept_parent
SET parent_concept_id = 3618
WHERE concept_id = 13690 AND parent_concept_id = 3701;

COMMIT;

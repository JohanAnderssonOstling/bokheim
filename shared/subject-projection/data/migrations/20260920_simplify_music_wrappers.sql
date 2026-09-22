-- Remove two selector-free navigation wrappers and combine the BISAC
-- history/criticism wrapper with the broader LCC music-literature wrapper.
-- Keep ID 782 and all classification coverage under a scope-appropriate name.
PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;

UPDATE concept_parent SET parent_concept_id=720
WHERE parent_concept_id=5656;

UPDATE concept_parent SET parent_concept_id=782
WHERE parent_concept_id=5947;

-- Detach the survivor before redirecting the merged subject's children.
UPDATE concept_parent SET parent_concept_id=720
WHERE concept_id=782 AND parent_concept_id=759;
UPDATE concept_parent SET parent_concept_id=782
WHERE parent_concept_id=759;

UPDATE bisac_selector SET concept_id=782 WHERE concept_id=759;
UPDATE concept SET preferred_label='Music Studies & Reference' WHERE concept_id=782;

DELETE FROM concept WHERE concept_id IN (5656,5947,759);
COMMIT;

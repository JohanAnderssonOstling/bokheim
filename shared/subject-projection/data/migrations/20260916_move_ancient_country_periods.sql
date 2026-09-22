-- Place country-specific ancient periods under their corresponding country
-- history branches.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

DELETE FROM concept_parent
WHERE concept_id = 3481 AND parent_concept_id = 3480;

INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal)
VALUES (3481,54716,0),(3482,54802,0);

DELETE FROM concept_parent
WHERE concept_id = 3482 AND parent_concept_id = 3480;

COMMIT;

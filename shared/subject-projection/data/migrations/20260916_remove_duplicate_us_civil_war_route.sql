-- Keep the century and military routes; remove the redundant direct era route.
PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;
DELETE FROM concept_parent WHERE concept_id=3408 AND parent_concept_id=3407;
COMMIT;

-- Keep the corresponding BISAC and LCC British periods in one English branch.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

UPDATE concept_parent
SET parent_concept_id = 54692
WHERE concept_id IN (3527, 3531, 3532, 3533)
  AND parent_concept_id = 11497;

COMMIT;

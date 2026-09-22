-- Rome is country-specific ancient history and belongs under Italian Ancient
-- Era rather than the general Eras/Ancient grouping.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

UPDATE concept_parent
SET parent_concept_id = 3571
WHERE concept_id = 3483 AND parent_concept_id = 3480;

COMMIT;

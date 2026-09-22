-- Consolidate broad U.S. biography entries under U.S. Biography.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

DELETE FROM concept_parent WHERE concept_id IN (8454,9008);
INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal)
VALUES (8454,51001,0),(9008,51001,1);

COMMIT;

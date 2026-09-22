-- Uralic and Basque languages belong with the language-family hierarchy,
-- not with Slavic & Eurasian literature.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

DELETE FROM concept_parent
WHERE concept_id=4479 OR parent_concept_id=4479;
INSERT INTO concept_parent (concept_id,parent_concept_id,ordinal)
VALUES (4479,5536,0);

COMMIT;

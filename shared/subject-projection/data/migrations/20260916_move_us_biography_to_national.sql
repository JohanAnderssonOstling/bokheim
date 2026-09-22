-- U.S. Biography belongs under the LCC National biography branch.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

UPDATE concept
SET preferred_label='U.S. Biography'
WHERE concept_id=51001 AND preferred_label='Biography';

INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT 51001,3221,COALESCE(MAX(ordinal)+1,0)
FROM concept_parent
WHERE parent_concept_id=3221;

DELETE FROM concept_parent
WHERE concept_id=51001 AND parent_concept_id=3402;

COMMIT;

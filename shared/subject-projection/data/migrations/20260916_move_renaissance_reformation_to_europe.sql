-- Renaissance & Reformation is a Europe-wide period, so keep one location
-- under European By Period.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

DELETE FROM concept_parent
WHERE concept_id = 10233 AND parent_concept_id IN (3618,8502);

INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal)
VALUES (10233,11498,0);

COMMIT;

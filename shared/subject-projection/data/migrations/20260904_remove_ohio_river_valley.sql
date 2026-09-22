-- Remove Ohio River and Valley detail from the curated United States regional
-- hierarchy. Full detail remains in the master taxonomy.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

CREATE TEMP TABLE ohio_valley_detail(concept_id TEXT PRIMARY KEY) WITHOUT ROWID;
WITH RECURSIVE detail(concept_id) AS (
 VALUES ('unified:d1c364b1-ce15-54be-b89a-db9679f792ad')
 UNION
 SELECT child.concept_id
 FROM detail parent
 JOIN concept_parent child ON child.parent_concept_id=parent.concept_id
)
INSERT INTO ohio_valley_detail SELECT concept_id FROM detail;

DELETE FROM concept_parent
WHERE concept_id IN (SELECT concept_id FROM ohio_valley_detail)
   OR parent_concept_id IN (SELECT concept_id FROM ohio_valley_detail);
DELETE FROM concept WHERE concept_id IN (SELECT concept_id FROM ohio_valley_detail);

DROP TABLE ohio_valley_detail;
COMMIT;

-- Make District of Columbia a leaf in the curated United States hierarchy.
-- Full local detail remains in the master taxonomy.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

CREATE TEMP TABLE district_detail(concept_id TEXT PRIMARY KEY) WITHOUT ROWID;
WITH RECURSIVE detail(concept_id) AS (
 SELECT child.concept_id
 FROM concept_parent child
 WHERE child.parent_concept_id='unified:f13d71fc-365a-5be4-a6d3-e891a8fc9b87'
 UNION
 SELECT child.concept_id
 FROM detail parent
 JOIN concept_parent child ON child.parent_concept_id=parent.concept_id
)
INSERT INTO district_detail SELECT concept_id FROM detail;

DELETE FROM concept_parent
WHERE concept_id IN (SELECT concept_id FROM district_detail)
   OR parent_concept_id IN (SELECT concept_id FROM district_detail);
DELETE FROM concept WHERE concept_id IN (SELECT concept_id FROM district_detail);

DROP TABLE district_detail;
COMMIT;

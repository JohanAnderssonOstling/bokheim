-- Remove ethnic and population grouping detail from the curated United States
-- history branch. Full detail remains in the master taxonomy.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

CREATE TEMP TABLE us_population_detail(concept_id TEXT PRIMARY KEY) WITHOUT ROWID;
WITH RECURSIVE detail(concept_id) AS (
 SELECT child.concept_id
 FROM concept_parent child
 JOIN concept c ON c.concept_id=child.concept_id
 WHERE child.parent_concept_id='unified:1fd4042d-5f0d-5835-ac17-9bc708487a6a'
   AND lower(c.preferred_label)='ethnic & population groups'
 UNION
 SELECT child.concept_id
 FROM detail parent
 JOIN concept_parent child ON child.parent_concept_id=parent.concept_id
)
INSERT INTO us_population_detail SELECT concept_id FROM detail;

DELETE FROM concept_parent
WHERE concept_id IN (SELECT concept_id FROM us_population_detail)
   OR parent_concept_id IN (SELECT concept_id FROM us_population_detail);
DELETE FROM concept WHERE concept_id IN (SELECT concept_id FROM us_population_detail);

DROP TABLE us_population_detail;
COMMIT;

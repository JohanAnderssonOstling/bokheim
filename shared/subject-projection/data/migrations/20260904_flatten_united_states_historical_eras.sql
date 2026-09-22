-- Make every entry in the curated United States Historical Eras branch a leaf.
-- This removes presidential administrations and all other finer era detail.
-- The general world-history Historical Eras branch is intentionally unchanged.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

CREATE TEMP TABLE us_era_detail(concept_id TEXT PRIMARY KEY) WITHOUT ROWID;
WITH RECURSIVE
era(concept_id) AS (
 SELECT child.concept_id
 FROM concept_parent child
 WHERE child.parent_concept_id='unified:3d1ef13b-9eea-570b-bc36-f12a5fe9f000'
),
detail(concept_id) AS (
 SELECT child.concept_id
 FROM era
 JOIN concept_parent child ON child.parent_concept_id=era.concept_id
 UNION
 SELECT child.concept_id
 FROM detail parent
 JOIN concept_parent child ON child.parent_concept_id=parent.concept_id
)
INSERT INTO us_era_detail SELECT concept_id FROM detail;

DELETE FROM concept_parent
WHERE concept_id IN (SELECT concept_id FROM us_era_detail)
   OR parent_concept_id IN (SELECT concept_id FROM us_era_detail);
DELETE FROM concept WHERE concept_id IN (SELECT concept_id FROM us_era_detail);

-- Also remove any presidential administration attached elsewhere in the U.S.
-- subtree (Lincoln's administration was a direct child of United States).
CREATE TEMP TABLE us_administration(concept_id TEXT PRIMARY KEY) WITHOUT ROWID;
WITH RECURSIVE
us(concept_id) AS (
 VALUES ('unified:1fd4042d-5f0d-5835-ac17-9bc708487a6a')
 UNION
 SELECT child.concept_id
 FROM concept_parent child
 JOIN us parent ON parent.concept_id=child.parent_concept_id
),
detail(concept_id) AS (
 SELECT item.concept_id
 FROM us item
 JOIN concept c ON c.concept_id=item.concept_id
 WHERE lower(c.preferred_label) LIKE '%''s administration%'
 UNION
 SELECT child.concept_id
 FROM detail parent
 JOIN concept_parent child ON child.parent_concept_id=parent.concept_id
)
INSERT INTO us_administration SELECT concept_id FROM detail;

DELETE FROM concept_parent
WHERE concept_id IN (SELECT concept_id FROM us_administration)
   OR parent_concept_id IN (SELECT concept_id FROM us_administration);
DELETE FROM concept WHERE concept_id IN (SELECT concept_id FROM us_administration);

DROP TABLE us_administration;
DROP TABLE us_era_detail;
COMMIT;

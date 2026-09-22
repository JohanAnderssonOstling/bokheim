-- Remove additional geographic and monument-detail branches throughout the
-- curated taxonomy. Full source detail remains in master.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

CREATE TEMP TABLE additional_geographic_detail(concept_id TEXT PRIMARY KEY) WITHOUT ROWID;
WITH RECURSIVE detail(concept_id) AS (
 SELECT concept_id FROM concept
 WHERE lower(preferred_label) IN (
   'history of two or more provinces',
   'place names, etc.',
   'historic buildings, castles, churches, etc.',
   'preservation of historic monuments, landmarks, scenery, etc.'
 )
 UNION
 SELECT child.concept_id
 FROM detail parent
 JOIN concept_parent child ON child.parent_concept_id=parent.concept_id
)
INSERT INTO additional_geographic_detail SELECT concept_id FROM detail;

DELETE FROM concept_parent
WHERE concept_id IN (SELECT concept_id FROM additional_geographic_detail)
   OR parent_concept_id IN (SELECT concept_id FROM additional_geographic_detail);
DELETE FROM concept WHERE concept_id IN (SELECT concept_id FROM additional_geographic_detail);

DROP TABLE additional_geographic_detail;
COMMIT;

-- Remove social-life/civilization and historic-monument facet branches
-- throughout the curated taxonomy. Full source detail remains in master.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

CREATE TEMP TABLE removed_facets(concept_id TEXT PRIMARY KEY) WITHOUT ROWID;
WITH RECURSIVE detail(concept_id) AS (
 SELECT concept_id FROM concept
 WHERE lower(preferred_label) LIKE 'social life and customs. civilization.%'
    OR lower(preferred_label) LIKE 'historic monuments, landmarks, scenery, etc.%'
 UNION
 SELECT child.concept_id
 FROM detail parent
 JOIN concept_parent child ON child.parent_concept_id=parent.concept_id
)
INSERT INTO removed_facets SELECT concept_id FROM detail;

DELETE FROM concept_parent
WHERE concept_id IN (SELECT concept_id FROM removed_facets)
   OR parent_concept_id IN (SELECT concept_id FROM removed_facets);
DELETE FROM concept WHERE concept_id IN (SELECT concept_id FROM removed_facets);

DROP TABLE removed_facets;
COMMIT;

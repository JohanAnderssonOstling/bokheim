-- Remove Cities & Towns grouping branches throughout the curated taxonomy.
-- The topical subject "Cities and towns in the arts" is intentionally not a
-- geographic grouping and remains. Full geographic detail remains in master.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

CREATE TEMP TABLE cities_towns_detail(concept_id TEXT PRIMARY KEY) WITHOUT ROWID;
WITH RECURSIVE detail(concept_id) AS (
 SELECT concept_id
 FROM concept
 WHERE lower(preferred_label) IN ('cities & towns','cities and towns')
 UNION
 SELECT child.concept_id
 FROM detail parent
 JOIN concept_parent child ON child.parent_concept_id=parent.concept_id
)
INSERT INTO cities_towns_detail SELECT concept_id FROM detail;

DELETE FROM concept_parent
WHERE concept_id IN (SELECT concept_id FROM cities_towns_detail)
   OR parent_concept_id IN (SELECT concept_id FROM cities_towns_detail);
DELETE FROM concept WHERE concept_id IN (SELECT concept_id FROM cities_towns_detail);

DROP TABLE cities_towns_detail;
COMMIT;

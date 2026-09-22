-- Remove Historiography and Ethnography branches throughout the curated
-- History taxonomy. Complete detail remains in the master taxonomy.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

CREATE TEMP TABLE history_historiography_ethnography(concept_id TEXT PRIMARY KEY) WITHOUT ROWID;
WITH RECURSIVE
history(concept_id) AS (
 VALUES ('bokheim:subject:history')
 UNION
 SELECT child.concept_id
 FROM concept_parent child
 JOIN history parent ON parent.concept_id=child.parent_concept_id
),
detail(concept_id) AS (
 SELECT item.concept_id
 FROM history item
 JOIN concept c ON c.concept_id=item.concept_id
 WHERE lower(c.preferred_label) IN ('historiography','ethnography')
 UNION
 SELECT child.concept_id
 FROM concept_parent child
 JOIN detail parent ON parent.concept_id=child.parent_concept_id
)
INSERT INTO history_historiography_ethnography SELECT concept_id FROM detail;

DELETE FROM concept_parent
WHERE concept_id IN (SELECT concept_id FROM history_historiography_ethnography)
   OR parent_concept_id IN (SELECT concept_id FROM history_historiography_ethnography);
DELETE FROM concept
WHERE concept_id IN (SELECT concept_id FROM history_historiography_ethnography);

DROP TABLE history_historiography_ethnography;
COMMIT;

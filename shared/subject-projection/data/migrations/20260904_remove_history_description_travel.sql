-- Remove Description and travel throughout the curated History tree.
-- Descriptive Geography is intentionally unaffected. Full detail remains in
-- the master taxonomy.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

CREATE TEMP TABLE history_description_travel(concept_id TEXT PRIMARY KEY) WITHOUT ROWID;
WITH RECURSIVE
history(concept_id) AS (
 VALUES ('bokheim:subject:history')
 UNION
 SELECT child.concept_id FROM concept_parent child
 JOIN history parent ON parent.concept_id=child.parent_concept_id
),
detail(concept_id) AS (
 SELECT item.concept_id FROM history item
 JOIN concept c ON c.concept_id=item.concept_id
 WHERE lower(c.preferred_label)='description and travel'
 UNION
 SELECT child.concept_id FROM concept_parent child
 JOIN detail parent ON parent.concept_id=child.parent_concept_id
)
INSERT INTO history_description_travel SELECT concept_id FROM detail;

DELETE FROM concept_parent
WHERE concept_id IN (SELECT concept_id FROM history_description_travel)
   OR parent_concept_id IN (SELECT concept_id FROM history_description_travel);
DELETE FROM concept WHERE concept_id IN (SELECT concept_id FROM history_description_travel);

DROP TABLE history_description_travel;
COMMIT;

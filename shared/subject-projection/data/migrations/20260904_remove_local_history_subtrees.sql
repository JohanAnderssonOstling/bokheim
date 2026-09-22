-- Remove local-history/place detail throughout the curated History tree.
-- Country periods and other deliberately curated country children outside
-- these branches remain untouched. Full detail remains in the master taxonomy.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

CREATE TEMP TABLE history_local_detail(concept_id TEXT PRIMARY KEY) WITHOUT ROWID;
WITH RECURSIVE
history(concept_id) AS (
 VALUES ('bokheim:subject:history')
 UNION
 SELECT child.concept_id
 FROM concept_parent child
 JOIN history parent ON parent.concept_id = child.parent_concept_id
),
detail(concept_id) AS (
 SELECT item.concept_id
 FROM history item
 JOIN concept c ON c.concept_id = item.concept_id
 WHERE lower(c.preferred_label) LIKE 'local history and description%'
    OR lower(c.preferred_label) = 'local history & places'
    OR lower(c.preferred_label) = 'by region & local history'
 UNION
 SELECT child.concept_id
 FROM concept_parent child
 JOIN detail parent ON parent.concept_id = child.parent_concept_id
)
INSERT INTO history_local_detail SELECT concept_id FROM detail;

DELETE FROM concept_parent
WHERE concept_id IN (SELECT concept_id FROM history_local_detail)
   OR parent_concept_id IN (SELECT concept_id FROM history_local_detail);
DELETE FROM concept WHERE concept_id IN (SELECT concept_id FROM history_local_detail);

DROP TABLE history_local_detail;
COMMIT;

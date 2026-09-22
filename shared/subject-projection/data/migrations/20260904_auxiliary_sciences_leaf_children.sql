-- Keep the five disciplines beneath Auxiliary Sciences of History as curated
-- leaves. Their detailed classifications remain in master-taxonomy-v2.sqlite3.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

CREATE TEMP TABLE auxiliary_science_detail_to_remove (
    concept_id TEXT PRIMARY KEY
) WITHOUT ROWID;

WITH RECURSIVE direct_child(concept_id) AS (
    SELECT concept_id
    FROM concept_parent
    WHERE parent_concept_id = 'unified:3cfd0be4-b1fe-599a-99d1-d79c4a585ea5'
), detail(concept_id) AS (
    SELECT child.concept_id
    FROM concept_parent child
    JOIN direct_child parent ON parent.concept_id = child.parent_concept_id
    UNION
    SELECT child.concept_id
    FROM concept_parent child
    JOIN detail parent ON parent.concept_id = child.parent_concept_id
)
INSERT INTO auxiliary_science_detail_to_remove
SELECT concept_id FROM detail;

DELETE FROM concept_parent
WHERE concept_id IN (SELECT concept_id FROM auxiliary_science_detail_to_remove)
   OR parent_concept_id IN (SELECT concept_id FROM auxiliary_science_detail_to_remove);

DELETE FROM concept
WHERE concept_id IN (SELECT concept_id FROM auxiliary_science_detail_to_remove);

DROP TABLE auxiliary_science_detail_to_remove;
COMMIT;

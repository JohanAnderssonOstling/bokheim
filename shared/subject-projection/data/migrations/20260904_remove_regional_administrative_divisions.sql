-- The curated taxonomy stops at meaningful regional and national divisions.
-- Detailed administrative geography remains in master-taxonomy-v2.sqlite3.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

CREATE TEMP TABLE regional_detail_to_remove (
    concept_id TEXT PRIMARY KEY
) WITHOUT ROWID;

WITH RECURSIVE regional_detail(concept_id) AS (
    SELECT concept_id
    FROM concept
    WHERE preferred_label = 'Regions & Administrative Divisions'
    UNION
    SELECT child.concept_id
    FROM concept_parent child
    JOIN regional_detail parent ON parent.concept_id = child.parent_concept_id
)
INSERT INTO regional_detail_to_remove
SELECT concept_id FROM regional_detail;

DELETE FROM concept_parent
WHERE concept_id IN (SELECT concept_id FROM regional_detail_to_remove)
   OR parent_concept_id IN (SELECT concept_id FROM regional_detail_to_remove);

DELETE FROM concept
WHERE concept_id IN (SELECT concept_id FROM regional_detail_to_remove);

DROP TABLE regional_detail_to_remove;
COMMIT;

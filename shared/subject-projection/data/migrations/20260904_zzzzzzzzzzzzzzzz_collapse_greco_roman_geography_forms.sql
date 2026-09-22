-- Keep Greco-Roman Geography as a useful subject, but remove its bibliographic
-- filing hierarchy by work coverage, author type, and publication period.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

CREATE TEMP TABLE geography_detail(concept_id TEXT PRIMARY KEY) WITHOUT ROWID;
WITH RECURSIVE detail(concept_id) AS (
    SELECT child.concept_id
    FROM concept_parent child
    WHERE child.parent_concept_id='unified:70b82b40-785f-59ff-977a-8e9eb6d04689'
    UNION
    SELECT child.concept_id
    FROM concept_parent child
    JOIN detail parent ON parent.concept_id=child.parent_concept_id
)
INSERT INTO geography_detail SELECT concept_id FROM detail;

WITH moved AS (
    SELECT s.system_id,s.selector,
           row_number() OVER (PARTITION BY s.system_id ORDER BY d.concept_id,s.ordinal) AS offset
    FROM geography_detail d
    JOIN source_selector s ON s.concept_id=d.concept_id
)
INSERT OR IGNORE INTO source_selector(concept_id,system_id,selector,ordinal)
SELECT 'unified:70b82b40-785f-59ff-977a-8e9eb6d04689',moved.system_id,moved.selector,
       COALESCE((SELECT max(existing.ordinal)+1
                 FROM source_selector existing
                 WHERE existing.concept_id='unified:70b82b40-785f-59ff-977a-8e9eb6d04689'
                   AND existing.system_id=moved.system_id),0)
       + moved.offset - 1
FROM moved;

DELETE FROM concept_parent
WHERE concept_id IN (SELECT concept_id FROM geography_detail)
   OR parent_concept_id IN (SELECT concept_id FROM geography_detail);

DELETE FROM concept
WHERE concept_id IN (SELECT concept_id FROM geography_detail)
  AND NOT EXISTS (
      SELECT 1 FROM concept_parent remaining
      WHERE remaining.concept_id=concept.concept_id
         OR remaining.parent_concept_id=concept.concept_id
  );

DROP TABLE geography_detail;
COMMIT;

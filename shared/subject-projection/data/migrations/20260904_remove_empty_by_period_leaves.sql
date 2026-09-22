-- Empty grouping labels are not useful as terminal browse destinations.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

CREATE TEMP TABLE empty_period_leaf(concept_id TEXT PRIMARY KEY) WITHOUT ROWID;
INSERT INTO empty_period_leaf
SELECT concept.concept_id
FROM concept
WHERE lower(trim(preferred_label))='by period'
  AND NOT EXISTS (
    SELECT 1 FROM source_selector
    WHERE source_selector.concept_id=concept.concept_id
  )
  AND NOT EXISTS (
    SELECT 1 FROM concept_parent
    WHERE concept_parent.parent_concept_id=concept.concept_id
  );

DELETE FROM concept_parent
WHERE concept_id IN (SELECT concept_id FROM empty_period_leaf);
DELETE FROM concept
WHERE concept_id IN (SELECT concept_id FROM empty_period_leaf);

DROP TABLE empty_period_leaf;
COMMIT;

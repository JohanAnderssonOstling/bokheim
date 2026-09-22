-- Remove terminal concepts that cannot receive any external-code match.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

CREATE TEMP TABLE selector_free_leaf(concept_id TEXT PRIMARY KEY) WITHOUT ROWID;
-- Work upward from every matchable concept. Anything outside this closure is
-- an entirely empty subtree, including parents exposed by removing dead leaves.
WITH RECURSIVE useful(concept_id) AS (
  SELECT DISTINCT concept_id FROM source_selector
  UNION
  SELECT edge.parent_concept_id
  FROM useful JOIN concept_parent edge ON edge.concept_id=useful.concept_id
)
INSERT INTO selector_free_leaf
SELECT concept_id FROM concept
WHERE concept_id NOT IN (SELECT concept_id FROM useful);

DELETE FROM concept_parent
WHERE concept_id IN (SELECT concept_id FROM selector_free_leaf)
   OR parent_concept_id IN (SELECT concept_id FROM selector_free_leaf);
DELETE FROM concept
WHERE concept_id IN (SELECT concept_id FROM selector_free_leaf);

DROP TABLE selector_free_leaf;
COMMIT;

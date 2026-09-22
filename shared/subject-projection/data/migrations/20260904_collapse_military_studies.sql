-- Keep Military Studies shallow in the curated taxonomy. Its main sections are
-- terminal leaves except Land Forces, whose five immediate branches remain as
-- leaves. Full classification detail remains in master-taxonomy-v2.sqlite3.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

CREATE TEMP TABLE military_boundary(concept_id TEXT PRIMARY KEY) WITHOUT ROWID;

-- All immediate Military Studies sections except Land Forces become leaves.
INSERT INTO military_boundary
SELECT child.concept_id
FROM concept military
JOIN concept_parent route ON route.parent_concept_id = military.concept_id
JOIN concept child ON child.concept_id = route.concept_id
WHERE military.preferred_label = 'Military Studies'
  AND child.preferred_label <> 'Land Forces';

-- Land Forces remains expandable, but each of its immediate branches is a leaf.
INSERT OR IGNORE INTO military_boundary
SELECT child.concept_id
FROM concept land
JOIN concept_parent route ON route.parent_concept_id = land.concept_id
JOIN concept child ON child.concept_id = route.concept_id
WHERE land.preferred_label = 'Land Forces';

CREATE TEMP TABLE military_detail(concept_id TEXT PRIMARY KEY) WITHOUT ROWID;
WITH RECURSIVE detail(concept_id) AS (
    SELECT child.concept_id
    FROM concept_parent child
    WHERE child.parent_concept_id IN (SELECT concept_id FROM military_boundary)
    UNION
    SELECT child.concept_id
    FROM concept_parent child
    JOIN detail parent ON parent.concept_id = child.parent_concept_id
)
INSERT INTO military_detail SELECT concept_id FROM detail;

-- Detach only the Military Studies detail routes. Concepts reused elsewhere
-- retain their unrelated parent routes and are not deleted.
DELETE FROM concept_parent
WHERE concept_id IN (SELECT concept_id FROM military_detail)
  AND parent_concept_id IN (
      SELECT concept_id FROM military_boundary
      UNION
      SELECT concept_id FROM military_detail
  );

-- Delete concepts made wholly unreachable by the pruning; retain multi-route
-- concepts that still belong elsewhere in the curated graph.
DELETE FROM concept
WHERE concept_id IN (SELECT concept_id FROM military_detail)
  AND NOT EXISTS (
      SELECT 1 FROM concept_parent remaining
      WHERE remaining.concept_id = concept.concept_id
         OR remaining.parent_concept_id = concept.concept_id
  );

DROP TABLE military_detail;
DROP TABLE military_boundary;
COMMIT;

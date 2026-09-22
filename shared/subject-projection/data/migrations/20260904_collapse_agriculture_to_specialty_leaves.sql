-- Keep Agriculture at three useful levels in the curated taxonomy:
-- Agriculture -> discipline -> specialized subject leaf. Full detailed
-- schedules remain available in master-taxonomy-v2.sqlite3.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

CREATE TEMP TABLE agriculture_context(concept_id TEXT PRIMARY KEY) WITHOUT ROWID;
INSERT INTO agriculture_context
SELECT agriculture.concept_id
FROM concept agriculture
JOIN concept_parent route ON route.concept_id = agriculture.concept_id
JOIN concept parent ON parent.concept_id = route.parent_concept_id
WHERE agriculture.preferred_label = 'Agriculture'
  AND parent.preferred_label = 'Applied Sciences & Technology';

CREATE TEMP TABLE agriculture_discipline(concept_id TEXT PRIMARY KEY) WITHOUT ROWID;
INSERT INTO agriculture_discipline
SELECT child.concept_id
FROM concept_parent child
WHERE child.parent_concept_id IN (SELECT concept_id FROM agriculture_context);

CREATE TEMP TABLE agriculture_specialty(concept_id TEXT PRIMARY KEY) WITHOUT ROWID;
INSERT INTO agriculture_specialty
SELECT child.concept_id
FROM concept_parent child
WHERE child.parent_concept_id IN (SELECT concept_id FROM agriculture_discipline);

CREATE TEMP TABLE agriculture_detail(concept_id TEXT PRIMARY KEY) WITHOUT ROWID;
WITH RECURSIVE detail(concept_id) AS (
    SELECT child.concept_id
    FROM concept_parent child
    WHERE child.parent_concept_id IN (SELECT concept_id FROM agriculture_specialty)
    UNION
    SELECT child.concept_id
    FROM concept_parent child
    JOIN detail parent ON parent.concept_id = child.parent_concept_id
)
INSERT INTO agriculture_detail SELECT concept_id FROM detail;

-- Detach only deep Agriculture routes. Concepts reused elsewhere retain their
-- unrelated placements.
DELETE FROM concept_parent
WHERE concept_id IN (SELECT concept_id FROM agriculture_detail)
  AND parent_concept_id IN (
      SELECT concept_id FROM agriculture_specialty
      UNION SELECT concept_id FROM agriculture_detail
  );

DELETE FROM concept
WHERE concept_id IN (SELECT concept_id FROM agriculture_detail)
  AND NOT EXISTS (
      SELECT 1 FROM concept_parent remaining
      WHERE remaining.concept_id = concept.concept_id
         OR remaining.parent_concept_id = concept.concept_id
  );

DROP TABLE agriculture_detail;
DROP TABLE agriculture_specialty;
DROP TABLE agriculture_discipline;
DROP TABLE agriculture_context;
COMMIT;

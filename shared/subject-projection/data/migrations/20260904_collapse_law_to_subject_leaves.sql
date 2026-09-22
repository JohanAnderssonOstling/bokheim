-- Keep two useful levels below Law: curated legal categories followed by
-- terminal subject leaves. Full detailed schedules remain in the master graph.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

CREATE TEMP TABLE law_category(concept_id TEXT PRIMARY KEY) WITHOUT ROWID;
INSERT INTO law_category
SELECT category.concept_id
FROM concept law
JOIN concept_parent route ON route.parent_concept_id = law.concept_id
JOIN concept category ON category.concept_id = route.concept_id
WHERE law.preferred_label = 'Law';

CREATE TEMP TABLE law_subject(concept_id TEXT PRIMARY KEY) WITHOUT ROWID;
INSERT INTO law_subject
SELECT child.concept_id
FROM concept_parent child
WHERE child.parent_concept_id IN (SELECT concept_id FROM law_category);

CREATE TEMP TABLE law_detail(concept_id TEXT PRIMARY KEY) WITHOUT ROWID;
WITH RECURSIVE detail(concept_id) AS (
    SELECT child.concept_id
    FROM concept_parent child
    WHERE child.parent_concept_id IN (SELECT concept_id FROM law_subject)
    UNION
    SELECT child.concept_id
    FROM concept_parent child
    JOIN detail parent ON parent.concept_id = child.parent_concept_id
)
INSERT INTO law_detail SELECT concept_id FROM detail;

-- Detach only routes below retained Law subjects. Concepts reused elsewhere
-- keep their unrelated routes.
DELETE FROM concept_parent
WHERE concept_id IN (SELECT concept_id FROM law_detail)
  AND parent_concept_id IN (
      SELECT concept_id FROM law_subject
      UNION SELECT concept_id FROM law_detail
  );

DELETE FROM concept
WHERE concept_id IN (SELECT concept_id FROM law_detail)
  AND NOT EXISTS (
      SELECT 1 FROM concept_parent remaining
      WHERE remaining.concept_id = concept.concept_id
         OR remaining.parent_concept_id = concept.concept_id
  );

DROP TABLE law_detail;
DROP TABLE law_subject;
DROP TABLE law_category;
COMMIT;

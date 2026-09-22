-- Flatten Venice's broad chronological wrappers in the curated taxonomy.
-- Periods and named events become direct children of Venice. Source structure
-- remains unchanged in master-taxonomy-v2.sqlite3.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

CREATE TEMP TABLE venice_context (
    venice_id TEXT PRIMARY KEY
) WITHOUT ROWID;

INSERT INTO venice_context
SELECT venice.concept_id
FROM concept venice
JOIN concept_parent placement ON placement.concept_id = venice.concept_id
JOIN concept italy ON italy.concept_id = placement.parent_concept_id
WHERE venice.preferred_label = 'Venice'
  AND italy.preferred_label = 'Italy';

CREATE TEMP TABLE venice_wrapper (
    concept_id TEXT PRIMARY KEY
) WITHOUT ROWID;

INSERT INTO venice_wrapper
SELECT child.concept_id
FROM concept_parent placement
JOIN concept child ON child.concept_id = placement.concept_id
WHERE placement.parent_concept_id IN (SELECT venice_id FROM venice_context)
  AND lower(child.preferred_label) IN ('early and medieval', 'modern');

-- Do not promote bibliographic detail or an individual person as periods.
CREATE TEMP TABLE venice_nonperiod (
    concept_id TEXT PRIMARY KEY
) WITHOUT ROWID;

WITH RECURSIVE unwanted(concept_id) AS (
    SELECT child.concept_id
    FROM venice_wrapper wrapper
    JOIN concept_parent route ON route.parent_concept_id = wrapper.concept_id
    JOIN concept child ON child.concept_id = route.concept_id
    WHERE lower(child.preferred_label) IN ('early works through 1800', 'paolo sarpi')
    UNION
    SELECT child.concept_id
    FROM concept_parent child
    JOIN unwanted parent ON parent.concept_id = child.parent_concept_id
)
INSERT INTO venice_nonperiod SELECT concept_id FROM unwanted;

-- Paolo Sarpi is nested beneath a period rather than directly beneath Modern.
INSERT OR IGNORE INTO venice_nonperiod
SELECT c.concept_id FROM concept c
WHERE lower(c.preferred_label) IN ('early works through 1800', 'paolo sarpi')
  AND EXISTS (
      WITH RECURSIVE ancestors(concept_id) AS (
          SELECT c.concept_id
          UNION
          SELECT p.parent_concept_id
          FROM concept_parent p JOIN ancestors a ON a.concept_id = p.concept_id
      )
      SELECT 1 FROM ancestors
      WHERE concept_id IN (SELECT venice_id FROM venice_context)
  );

DELETE FROM concept_parent
WHERE concept_id IN (SELECT concept_id FROM venice_nonperiod)
   OR parent_concept_id IN (SELECT concept_id FROM venice_nonperiod);
DELETE FROM concept
WHERE concept_id IN (SELECT concept_id FROM venice_nonperiod);

CREATE TEMP TABLE promoted AS
SELECT route.concept_id,
       context.venice_id AS parent_concept_id,
       route.display_order
FROM venice_wrapper wrapper
JOIN concept_parent route ON route.parent_concept_id = wrapper.concept_id
CROSS JOIN venice_context context
WHERE route.concept_id NOT IN (SELECT concept_id FROM venice_nonperiod);

INSERT INTO concept_parent(concept_id, parent_concept_id, ordinal, display_order)
SELECT promoted.concept_id,
       promoted.parent_concept_id,
       COALESCE((SELECT max(existing.ordinal) + 1
                 FROM concept_parent existing
                 WHERE existing.concept_id = promoted.concept_id), 0),
       promoted.display_order
FROM promoted
WHERE NOT EXISTS (
    SELECT 1 FROM concept_parent existing
    WHERE existing.concept_id = promoted.concept_id
      AND existing.parent_concept_id = promoted.parent_concept_id
);

DELETE FROM concept_parent
WHERE concept_id IN (SELECT concept_id FROM venice_wrapper)
   OR parent_concept_id IN (SELECT concept_id FROM venice_wrapper);
DELETE FROM concept
WHERE concept_id IN (SELECT concept_id FROM venice_wrapper);

DROP TABLE promoted;
DROP TABLE venice_nonperiod;
DROP TABLE venice_wrapper;
DROP TABLE venice_context;
COMMIT;

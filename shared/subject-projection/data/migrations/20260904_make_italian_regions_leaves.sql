-- Italian regional place concepts are terminal leaves in the curated taxonomy.
-- Their detailed period structure remains in master-taxonomy-v2.sqlite3.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

CREATE TEMP TABLE italian_regional_descendant (
    concept_id TEXT PRIMARY KEY
) WITHOUT ROWID;

WITH RECURSIVE
italian_region(concept_id) AS (
    SELECT region.concept_id
    FROM concept italy
    JOIN concept_parent route ON route.parent_concept_id = italy.concept_id
    JOIN concept region ON region.concept_id = route.concept_id
    WHERE italy.preferred_label = 'Italy'
      AND region.preferred_label IN (
          'Genoa',
          'Milan. Lombardy',
          'Naples. Kingdom of the Two Sicilies',
          'Papal States (States of the Church). Holy See. Vatican City',
          'Piedmont. Savoy',
          'Rome (Modern city)',
          'Sicily',
          'Southern Italy',
          'Tuscany. Florence',
          'Venice'
      )
),
descendant(concept_id) AS (
    SELECT child.concept_id
    FROM concept_parent child
    WHERE child.parent_concept_id IN (SELECT concept_id FROM italian_region)
    UNION
    SELECT child.concept_id
    FROM concept_parent child
    JOIN descendant parent ON parent.concept_id = child.parent_concept_id
)
INSERT INTO italian_regional_descendant
SELECT concept_id FROM descendant;

DELETE FROM concept_parent
WHERE concept_id IN (SELECT concept_id FROM italian_regional_descendant)
   OR parent_concept_id IN (SELECT concept_id FROM italian_regional_descendant);

DELETE FROM concept
WHERE concept_id IN (SELECT concept_id FROM italian_regional_descendant);

DROP TABLE italian_regional_descendant;
COMMIT;

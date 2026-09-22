-- Keep Italy's historical place and period navigation focused on chronology.
-- Remove architectural, infrastructure, descriptive, monument, preservation,
-- and other local-interest subtrees from the curated taxonomy only. The full
-- source graph remains available in master-taxonomy-v2.sqlite3.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

CREATE TEMP TABLE italy_nonperiod_detail (
    concept_id TEXT PRIMARY KEY
) WITHOUT ROWID;

WITH RECURSIVE
italy(concept_id) AS (
    SELECT child.concept_id
    FROM concept_parent child
    JOIN concept italy_concept ON italy_concept.concept_id = child.concept_id
    JOIN concept parent_concept ON parent_concept.concept_id = child.parent_concept_id
    WHERE italy_concept.preferred_label = 'Italy'
      AND parent_concept.preferred_label = 'Southern Europe'
      AND EXISTS (
          SELECT 1 FROM concept_parent descendant
          WHERE descendant.parent_concept_id = child.concept_id
      )
    UNION
    SELECT child.concept_id
    FROM concept_parent child
    JOIN italy parent ON parent.concept_id = child.parent_concept_id
),
detail(concept_id) AS (
    SELECT item.concept_id
    FROM italy item
    JOIN concept c ON c.concept_id = item.concept_id
    WHERE lower(c.preferred_label) IN (
        'antiquities',
        'antiquities (medieval, etc.)',
        'antiquities. civilization. culture',
        'buildings',
        'canals',
        'cemeteries',
        'description',
        'foreign and general relations',
        'fountains',
        'general history',
        'geography. description and travel',
        'historical geography',
        'history of travel',
        'houses, noble families, etc.',
        'historic monuments, landmarks, etc.',
        'historic monuments, landmarks, etc. (general)',
        'individual colonies',
        'islands of italy (collectively)',
        'italian colonies (former)',
        'local',
        'local (florence)',
        'lagoons (general)',
        'military and naval history',
        'military history',
        'monumental and picturesque, etc.',
        'monuments, statues, etc.',
        'parks, squares, cemeteries, etc.',
        'parks, squares, etc.',
        'political history',
        'preservation',
        'preservation of historic monuments, etc.',
        'preservation of historic monuments, landmarks, scenery, etc.',
        'river front (tiber)',
        'social life and customs. civilization',
        'social life and customs. civilization. intellectual life',
        'streets. bridges',
        'streets, bridges, gates, etc.',
        'syllabi, etc.'
    )
    UNION
    SELECT child.concept_id
    FROM concept_parent child
    JOIN detail parent ON parent.concept_id = child.parent_concept_id
)
INSERT INTO italy_nonperiod_detail
SELECT concept_id FROM detail;

DELETE FROM concept_parent
WHERE concept_id IN (SELECT concept_id FROM italy_nonperiod_detail)
   OR parent_concept_id IN (SELECT concept_id FROM italy_nonperiod_detail);

DELETE FROM concept
WHERE concept_id IN (SELECT concept_id FROM italy_nonperiod_detail);

DROP TABLE italy_nonperiod_detail;
COMMIT;

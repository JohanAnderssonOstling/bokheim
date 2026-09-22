-- Remove terminal catalog headings that describe publication or study form
-- rather than subject matter. Non-leaf headings and substantive topics such
-- as History of Bibliography are intentionally preserved.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

CREATE TEMP TABLE publication_form_leaf(concept_id TEXT PRIMARY KEY) WITHOUT ROWID;

INSERT INTO publication_form_leaf
SELECT concept.concept_id
FROM concept
WHERE NOT EXISTS (
    SELECT 1 FROM concept_parent child
    WHERE child.parent_concept_id = concept.concept_id
)
AND (
       lower(concept.preferred_label) LIKE 'periodicals%'
    OR lower(concept.preferred_label) LIKE 'bibliograph%'
    OR lower(concept.preferred_label) LIKE 'biobibliograph%'
    OR lower(concept.preferred_label) IN ('general bibliography', 'reference works & bibliography')
    OR lower(concept.preferred_label) LIKE '%textbook%'
    OR lower(concept.preferred_label) LIKE '%outlines, syllabi%'
    OR lower(concept.preferred_label) LIKE '%problems, exercises, examinations%'
    OR lower(concept.preferred_label) IN (
        'dictionaries & encyclopedias',
        'dictionaries and encyclopedias',
        'dictionaries. encyclopedias',
        'dictionaries',
        'encyclopedias',
        'collected accounts of events',
        'miscellany of history',
        'miscellany',
        'special topics'
    )
);

DELETE FROM concept_parent
WHERE concept_id IN (SELECT concept_id FROM publication_form_leaf);

DELETE FROM concept
WHERE concept_id IN (SELECT concept_id FROM publication_form_leaf);

DROP TABLE publication_form_leaf;
COMMIT;

-- Simplify Literature around unified semantic branches rather than external
-- classification-system boundaries. Existing selector-bearing concepts and
-- their detailed descendants are preserved. This migration is idempotent.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

UPDATE concept
SET preferred_label = 'Literary Criticism & Theory'
WHERE concept_id = 'bokheim:subject:literature:theory-criticism';

UPDATE concept
SET preferred_label = 'Literature Reference & Study'
WHERE concept_id = 'bokheim:subject:literature:reference-education';

INSERT OR IGNORE INTO concept(concept_id, preferred_label) VALUES
    ('bokheim:subject:literature:world-regional', 'World & Regional Literature'),
    ('bokheim:subject:literature:language-based', 'Language-Based Literatures');

INSERT OR IGNORE INTO concept_parent(concept_id, parent_concept_id, ordinal) VALUES
    ('bokheim:subject:literature:world-regional', 'unified:6aaa3a4d-bf1d-5bb0-8cda-617aff1c5854', 0),
    ('bokheim:subject:literature:language-based', 'unified:6aaa3a4d-bf1d-5bb0-8cda-617aff1c5854', 0);

-- Regional branches retain their existing internal mix of BISAC, DDC, and
-- LCC coverage beneath one unified World & Regional Literature branch.
UPDATE concept_parent
SET parent_concept_id = 'bokheim:subject:literature:world-regional'
WHERE parent_concept_id = 'unified:6aaa3a4d-bf1d-5bb0-8cda-617aff1c5854'
  AND concept_id IN (
      'unified:c0fdaa85-90b6-5247-97b2-00be9436df34',
      'bokheim:literature:world:americas',
      'bokheim:subject:literature:asian-pacific',
      'bokheim:subject:literature:african-middle-eastern',
      'bokheim:subject:literature:indigenous-other'
  );

-- Language and literary-tradition branches are grouped without changing
-- their underlying concepts, descendants, or source mappings.
UPDATE concept_parent
SET parent_concept_id = 'bokheim:subject:literature:language-based'
WHERE parent_concept_id = 'unified:6aaa3a4d-bf1d-5bb0-8cda-617aff1c5854'
  AND concept_id IN (
      'bokheim:subject:literature:english',
      'bokheim:subject:literature:romance',
      'bokheim:subject:literature:germanic',
      'bokheim:subject:literature:classical',
      'bokheim:subject:literature:slavic-eurasian'
  );

COMMIT;

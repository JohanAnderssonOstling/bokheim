-- Collapse two selector-free genre wrappers and expose their selector-backed
-- children directly beneath Literature > Genres. This migration is idempotent.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

-- Prefer any equivalent direct edge that may already exist.
DELETE FROM concept_parent
WHERE parent_concept_id IN (
    'bokheim:fiction:mystery-suspense',
    'bokheim:fiction:speculative'
)
  AND EXISTS (
      SELECT 1
      FROM concept_parent existing
      WHERE existing.concept_id = concept_parent.concept_id
        AND existing.parent_concept_id = 'bokheim:subject:literature:genres'
  );

UPDATE concept_parent
SET parent_concept_id = 'bokheim:subject:literature:genres'
WHERE parent_concept_id IN (
    'bokheim:fiction:mystery-suspense',
    'bokheim:fiction:speculative'
);

DELETE FROM concept
WHERE concept_id IN (
    'bokheim:fiction:mystery-suspense',
    'bokheim:fiction:speculative'
);

COMMIT;

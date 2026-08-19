-- Collapse the selector-free General Literature > By Period wrapper and expose
-- its two selector-backed periods directly. Existing edge orders are retained.
-- This migration is intentionally idempotent.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

DELETE FROM concept_parent
WHERE parent_concept_id = 'unified:06644ff9-4ffe-5bb2-aff6-98051796c4bc'
  AND EXISTS (
      SELECT 1
      FROM concept_parent existing
      WHERE existing.concept_id = concept_parent.concept_id
        AND existing.parent_concept_id = 'unified:bbcf0a3c-e484-5778-ab84-70596d1c59c9'
  );

UPDATE concept_parent
SET parent_concept_id = 'unified:bbcf0a3c-e484-5778-ab84-70596d1c59c9'
WHERE parent_concept_id = 'unified:06644ff9-4ffe-5bb2-aff6-98051796c4bc';

DELETE FROM concept
WHERE concept_id = 'unified:06644ff9-4ffe-5bb2-aff6-98051796c4bc';

COMMIT;

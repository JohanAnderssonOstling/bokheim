-- Split the Work & Careers umbrella into direct Work and Careers branches
-- beneath Economics & Business. Existing edge display orders are preserved.
-- This migration is intentionally idempotent.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

UPDATE concept
SET preferred_label = 'Work'
WHERE concept_id = 'bokheim:subject:economics:workplace';

UPDATE concept_parent
SET parent_concept_id = 'unified:103d333e-4f6d-5d1c-9c4b-950677c8d681'
WHERE concept_id = 'unified:ecfd77fa-ee5f-523c-8310-85742e6d49ec'
  AND parent_concept_id = 'bokheim:subject:economics:workplace';

COMMIT;

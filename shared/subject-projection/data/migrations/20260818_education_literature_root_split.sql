-- Promote Education, Study Aids, and Literature to independent taxonomy roots.
-- The empty Education & Learning wrapper is removed; Humanities retains its
-- other branches. This migration is intentionally idempotent.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

-- Split Education & Learning into its two selector-backed roots.
DELETE FROM concept_parent
WHERE concept_id IN (
    'unified:d2ff67fe-cb2c-5ef6-942e-ca88abfa6289',
    'unified:a95070f7-c933-5053-966b-d6ab76586f64'
)
  AND parent_concept_id = 'unified:bcc1c1d4-097f-5f7e-bcf3-d7225b2a4ba9';

DELETE FROM concept
WHERE concept_id = 'unified:bcc1c1d4-097f-5f7e-bcf3-d7225b2a4ba9';

-- Literature is a substantial selector-backed branch and stands as its own
-- root rather than remaining beneath the unclassified Humanities wrapper.
DELETE FROM concept_parent
WHERE concept_id = 'unified:6aaa3a4d-bf1d-5bb0-8cda-617aff1c5854'
  AND parent_concept_id = 'bokheim:subject:humanities';

COMMIT;

-- Keep Finance directly beneath Economics & Business and reconcile its
-- general banking entries. Context-specific Banking concepts beneath Law are
-- intentionally unchanged. This migration is intentionally idempotent.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

-- Finance is already directly beneath Economics & Business; remove its
-- redundant Commerce / Business route.
DELETE FROM concept_parent
WHERE concept_id = 'unified:66cf212d-88c2-5e3a-9cd3-7b4d76217132'
  AND parent_concept_id = 'unified:df4df9ce-4492-500d-a82d-1ce50c92f18b';

-- Merge BISAC Banks & Banking into the detailed LCC Banking concept.
INSERT OR IGNORE INTO source_selector(concept_id, system_id, selector, ordinal) VALUES
    ('unified:14251312-253e-5e28-9505-ee4431a5a298', 'bisac', 'BUS004000', 0),
    ('unified:14251312-253e-5e28-9505-ee4431a5a298', 'bisac', 'BUS112000', 1);

DELETE FROM concept
WHERE concept_id IN (
    'unified:23c07efb-fccb-56b4-88cd-6f8cbdbe3b11',
    'unified:1438360a-b0e5-5744-8634-90f1c188cc60'
);

COMMIT;

-- Correct the Applied / Clinical Psychology hierarchy. Applied Psychology is
-- broader; the duplicate Clinical Psychology concept is folded into the
-- curated Clinical Psychology branch, and Psychotherapy is placed beneath it.
-- This migration is intentionally idempotent.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

-- Preserve the duplicate concept's BISAC coverage on canonical Clinical
-- Psychology before removing it.
INSERT OR IGNORE INTO source_selector(concept_id, system_id, selector, ordinal) VALUES
    ('bokheim:psychology:clinical-applied', 'bisac', 'PSY007000', 0);

-- Reverse the incorrectly nested umbrella relationship.
DELETE FROM concept_parent
WHERE concept_id = 'unified:51a03e78-a8f8-5d69-a963-aed4ce205e5d'
  AND parent_concept_id = 'bokheim:psychology:clinical-applied';

DELETE FROM concept_parent
WHERE concept_id = 'bokheim:psychology:clinical-applied'
  AND parent_concept_id = 'unified:91e9ffa6-f917-5bc8-83c8-c0c5933abcd3';

INSERT OR IGNORE INTO concept_parent(concept_id, parent_concept_id, ordinal, display_order) VALUES
    ('unified:51a03e78-a8f8-5d69-a963-aed4ce205e5d', 'unified:91e9ffa6-f917-5bc8-83c8-c0c5933abcd3', 0, 50);

-- Remove the duplicate Clinical Psychology concept and attach the canonical
-- branch beneath Applied Psychology.
DELETE FROM concept
WHERE concept_id = 'unified:9657ea4c-edd0-52e5-b73f-da5526bfc5ea';

INSERT OR IGNORE INTO concept_parent(concept_id, parent_concept_id, ordinal, display_order) VALUES
    ('bokheim:psychology:clinical-applied', 'unified:51a03e78-a8f8-5d69-a963-aed4ce205e5d', 0, 0);

-- Psychotherapy is specifically clinical rather than a peer of Clinical
-- Psychology within the broader applied field.
DELETE FROM concept_parent
WHERE concept_id = 'unified:dfc33620-db1a-5369-907e-1b93d9f2857a'
  AND parent_concept_id = 'unified:51a03e78-a8f8-5d69-a963-aed4ce205e5d';

INSERT OR IGNORE INTO concept_parent(concept_id, parent_concept_id, ordinal, display_order) VALUES
    ('unified:dfc33620-db1a-5369-907e-1b93d9f2857a', 'bokheim:psychology:clinical-applied', 0, 0);

COMMIT;

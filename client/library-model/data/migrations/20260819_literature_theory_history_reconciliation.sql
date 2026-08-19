-- Remove the selector-free Theory, History & Criticism wrapper. Literary
-- History becomes direct, while DDC and LCC literary theory are unified with
-- all detailed children preserved. This migration is idempotent.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

-- Move Literary History directly beneath Literary Criticism & Theory.
UPDATE concept_parent
SET parent_concept_id = 'bokheim:subject:literature:theory-criticism'
WHERE concept_id = 'unified:70b3087b-1a72-5fae-b1e6-97b9b7f739c2'
  AND parent_concept_id = 'bokheim:subject:literature:general:theory-history';

UPDATE concept
SET preferred_label = 'Literary History'
WHERE concept_id = 'unified:70b3087b-1a72-5fae-b1e6-97b9b7f739c2';

-- Merge the detailed LCC theory concept into the existing DDC Literary Theory.
INSERT OR IGNORE INTO source_selector(concept_id, system_id, selector, ordinal) VALUES
    ('unified:3a7aa472-aa2e-5910-9fd8-9a3ef391ad07', 'lcc', 'PN45..PN57@2', 0);

UPDATE concept_parent
SET parent_concept_id = 'unified:3a7aa472-aa2e-5910-9fd8-9a3ef391ad07'
WHERE parent_concept_id = 'unified:9e9715d4-e8a2-5443-bb75-a327caae9dfa';

DELETE FROM concept
WHERE concept_id = 'unified:9e9715d4-e8a2-5443-bb75-a327caae9dfa';

DELETE FROM concept
WHERE concept_id = 'bokheim:subject:literature:general:theory-history';

COMMIT;

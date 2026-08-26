-- Reconcile the second reviewed batch of duplicate Social Sciences concepts.
-- Context-specific Motivation and Commerce concepts elsewhere in the tree are
-- intentionally left untouched. This migration is idempotent.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

-- Evolutionary Psychology: retain the direct Psychology entry and fold the
-- nested Development, Difference & Evolution entry into it.
INSERT OR IGNORE INTO source_selector(concept_id, system_id, selector, ordinal) VALUES
    ('bokheim:philosophy-lcc-full:532ce520-3f04-5525-92f9-fded16481a98', 'bisac', 'PSY053000', 0);

DELETE FROM concept
WHERE concept_id = 'unified:ff592d62-794d-541d-a98e-92aa14c27b5d';

-- Motivation: retain the entry beneath Personality, Emotion & Motivation.
INSERT OR IGNORE INTO source_selector(concept_id, system_id, selector, ordinal) VALUES
    ('unified:b3c97305-1d18-5cd7-94ce-29d6f2271f9d', 'lcc', 'BF501..BF504.3@2', 1);

INSERT OR IGNORE INTO source_label(concept_id, label, ordinal) VALUES
    ('unified:b3c97305-1d18-5cd7-94ce-29d6f2271f9d', 'Motivation', 0);

DELETE FROM concept
WHERE concept_id = 'bokheim:philosophy-lcc-full:eb9c6c95-d6b4-500b-9ef4-0a61f58f6486';

-- Folklore: retain the direct Anthropology entry, including the detailed LCC
-- children previously attached through Anthropology Fields.
UPDATE concept_parent
SET parent_concept_id = 'unified:014392ef-28b9-5528-bfd0-9fdb93d2d7cf'
WHERE parent_concept_id = 'unified:e7264015-badd-5538-84d7-7e5932a93722';

INSERT OR IGNORE INTO source_selector(concept_id, system_id, selector, ordinal) VALUES
    ('unified:014392ef-28b9-5528-bfd0-9fdb93d2d7cf', 'lcc', 'GR', 0),
    ('unified:014392ef-28b9-5528-bfd0-9fdb93d2d7cf', 'lcc', 'GR1..GR950@1', 1);

DELETE FROM concept
WHERE concept_id = 'unified:e7264015-badd-5538-84d7-7e5932a93722';

-- Commerce: retain the direct Economics & Business entry and fold in the
-- BISAC selector from the Trade & Transportation duplicate.
INSERT OR IGNORE INTO source_selector(concept_id, system_id, selector, ordinal) VALUES
    ('unified:98c6ca29-056c-5b82-83e7-aebaaa8169f9', 'bisac', 'BUS073000', 0);

DELETE FROM concept
WHERE concept_id = 'unified:907cb371-dae8-5f37-b362-71d953cb5ac6';

-- Secret Societies: retain the Sociology entry and remove the ambiguous Social
-- Welfare & Services route while preserving its DDC coverage.
INSERT OR IGNORE INTO source_selector(concept_id, system_id, selector, ordinal) VALUES
    ('unified:5133b8d0-1fdd-5dbf-93ad-f794a89f13ed', 'ddc', '366', 0);

DELETE FROM concept
WHERE concept_id = 'unified:2989b21c-bc12-5f51-be30-a941d7b37948';

COMMIT;

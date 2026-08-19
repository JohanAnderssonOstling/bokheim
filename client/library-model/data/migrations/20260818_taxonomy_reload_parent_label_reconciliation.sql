-- Remove direct parent-child concepts with identical preferred labels. These
-- relationships are rejected by the runtime taxonomy installer.
-- This migration is intentionally idempotent.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

-- Entrepreneurship: retain the established parent and preserve BISAC coverage.
INSERT OR IGNORE INTO source_selector(concept_id, system_id, selector, ordinal) VALUES
    ('bokheim:subject:economics:entrepreneurship', 'bisac', 'BUS025000', 0);

DELETE FROM concept
WHERE concept_id = 'unified:58476e3a-4a7c-51f9-8ff9-e8fe6c2d1356';

-- International Relations: retain the curated parent, move the detailed
-- children onto it, and preserve all BISAC, DDC, and LCC selectors.
UPDATE concept_parent
SET parent_concept_id = 'bokheim:politics:international-global'
WHERE parent_concept_id = 'unified:72820ea6-bb74-5926-ac0b-73ce2e38c16c';

INSERT OR IGNORE INTO source_selector(concept_id, system_id, selector, ordinal) VALUES
    ('bokheim:politics:international-global', 'bisac', 'POL011000', 0),
    ('bokheim:politics:international-global', 'ddc', '327', 0),
    ('bokheim:politics:international-global', 'lcc', 'JZ', 0),
    ('bokheim:politics:international-global', 'lcc', 'JZ5..JZ6530@1', 1);

DELETE FROM concept
WHERE concept_id = 'unified:72820ea6-bb74-5926-ac0b-73ce2e38c16c';

COMMIT;

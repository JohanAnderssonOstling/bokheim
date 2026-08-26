-- Reconcile the third reviewed batch of duplicate Social Sciences concepts.
-- Shared interdisciplinary concepts retain each approved parent. Contextual
-- Advertising concepts receive distinct labels. This migration is idempotent.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

-- Economic Policy: share the Economics & Business concept with Public Policy.
INSERT OR IGNORE INTO source_selector(concept_id, system_id, selector, ordinal) VALUES
    ('bokheim:subject:economics:policy-development', 'bisac', 'POL024000', 0);

INSERT OR IGNORE INTO concept_parent(concept_id, parent_concept_id, ordinal, display_order) VALUES
    ('bokheim:subject:economics:policy-development', 'unified:40b5c498-0b47-5d4b-88d4-d1f15c11a199', 1, 0);

DELETE FROM concept
WHERE concept_id = 'unified:6d17d34f-2f23-5b85-8dc4-4de73c60c38c';

-- The law-specific concept is regulation rather than the general policy topic.
UPDATE concept
SET preferred_label = 'Economic Regulation'
WHERE concept_id = 'unified:6d4cdc08-4572-5bfd-82f8-b4313b53f6fd';

-- Civil Rights: use one concept beneath both Politics and Law.
INSERT OR IGNORE INTO source_selector(concept_id, system_id, selector, ordinal) VALUES
    ('unified:d292d3d5-5619-561d-b79e-6ccec11024ba', 'bisac', 'LAW013000', 1);

INSERT OR IGNORE INTO concept_parent(concept_id, parent_concept_id, ordinal, display_order) VALUES
    ('unified:d292d3d5-5619-561d-b79e-6ccec11024ba', 'bokheim:law:public:constitutional-rights', 1, 8);

DELETE FROM concept
WHERE concept_id = 'unified:7cb4feac-6721-5855-bb5d-287832a0e0f2';

-- Social Psychology: use the detailed Sociology concept beneath both fields.
INSERT OR IGNORE INTO source_selector(concept_id, system_id, selector, ordinal) VALUES
    ('unified:934baa6a-2143-5727-934c-bb18fa5821b1', 'bisac', 'PSY031000', 0);

INSERT OR IGNORE INTO concept_parent(concept_id, parent_concept_id, ordinal, display_order) VALUES
    ('unified:934baa6a-2143-5727-934c-bb18fa5821b1', 'bokheim:psychology:social-cultural', 1, 0);

UPDATE concept
SET preferred_label = 'Social Psychology'
WHERE concept_id = 'unified:934baa6a-2143-5727-934c-bb18fa5821b1';

DELETE FROM concept
WHERE concept_id = 'unified:cd11d16e-f3c3-501d-9e9d-f00811a559ff';

-- Autism Spectrum Disorders: share the Psychology concept with the relevant
-- Family category.
INSERT OR IGNORE INTO source_selector(concept_id, system_id, selector, ordinal) VALUES
    ('unified:2851df81-8404-5001-97d5-86823fa64682', 'bisac', 'FAM048000', 1);

INSERT OR IGNORE INTO concept_parent(concept_id, parent_concept_id, ordinal, display_order) VALUES
    ('unified:2851df81-8404-5001-97d5-86823fa64682', 'unified:6f8050db-6db4-5317-b698-a9a0ae27947b', 1, 0);

DELETE FROM concept
WHERE concept_id = 'unified:47182535-72fa-5261-8daa-7f9d3e0f1b5f';

-- Advertising: retain the general Marketing label and qualify the Design and
-- Culture concepts. Merge the two legal concepts under a distinct label.
UPDATE concept
SET preferred_label = 'Advertising Design'
WHERE concept_id = 'unified:42a7ffe6-2e4e-57ce-80f9-ac494dd9feb2';

UPDATE concept
SET preferred_label = 'Advertising & Culture'
WHERE concept_id = 'unified:97a8541a-a32b-59e1-bddd-7aaa05ac8c97';

UPDATE concept
SET preferred_label = 'Advertising Law'
WHERE concept_id = 'bokheim:lcc-detail:34a5c641-0f1e-5e9d-91d7-e2e9d8ce9efb';

INSERT OR IGNORE INTO source_selector(concept_id, system_id, selector, ordinal) VALUES
    ('bokheim:lcc-detail:34a5c641-0f1e-5e9d-91d7-e2e9d8ce9efb', 'lcc', 'KBM1237..KBM1237@4', 8);

INSERT OR IGNORE INTO concept_parent(concept_id, parent_concept_id, ordinal, display_order) VALUES
    ('bokheim:lcc-detail:34a5c641-0f1e-5e9d-91d7-e2e9d8ce9efb', 'unified:da953ff6-d761-5666-8273-95cead195c3c', 2, 10);

DELETE FROM concept
WHERE concept_id = 'bokheim:lcc-detail:ac5f4c74-607f-53a1-9502-8ee50d2183a7';

COMMIT;

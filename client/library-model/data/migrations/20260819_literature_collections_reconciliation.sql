-- Reconcile General Literature collections with the existing unified literary
-- collections, forms, and reference concepts. Detailed LCC coverage and leaf
-- categories are preserved. This migration is intentionally idempotent.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

-- Preserve the broad LCC collection ranges on Literary Collections.
INSERT OR IGNORE INTO source_selector(concept_id, system_id, selector, ordinal)
SELECT 'unified:aeec07de-7d62-59b5-90c5-f775cf9f888c', system_id, selector, ordinal
FROM source_selector
WHERE concept_id = 'unified:ed1cc769-f28a-59df-b83a-4e2b81540b37';

-- Merge equivalent collection forms across LCC and BISAC.
INSERT OR IGNORE INTO source_selector(concept_id, system_id, selector, ordinal) VALUES
    ('unified:7f34a373-09e9-5e61-a9f2-69a11350098d', 'lcc', 'PN6141..PN6145@4', 0),
    ('unified:0359f62a-0aa4-5708-aa76-05ec604b0bca', 'lcc', 'PN6130..PN6140@4', 0),
    ('unified:83aaca11-1f04-5767-8a02-5ad521886cb7', 'lcc', 'PN4390..PN4390@2', 0),
    ('unified:85dd3fd2-15b0-58a3-b2c6-27e36e218196', 'lcc', 'PN6080..PN6095@4', 0),
    ('unified:85dd3fd2-15b0-58a3-b2c6-27e36e218196', 'lcc', 'PN6081..PN6084@4', 1),
    ('unified:85dd3fd2-15b0-58a3-b2c6-27e36e218196', 'lcc', 'PN6086..PN6089@4', 2),
    ('unified:85dd3fd2-15b0-58a3-b2c6-27e36e218196', 'lcc', 'PN6090..PN6093@4', 3),
    ('unified:84143bb3-fe56-5ec4-a2b3-5e4b6a3fe45d', 'lcc', 'PN6110.5..PN6120@4', 0),
    ('unified:373583c3-8d1b-5a8f-b52c-c6414ea34808', 'lcc', 'PN6099..PN6110@4', 0),
    ('unified:f359eef0-cb25-55ac-ba46-8b3ac8e4a40c', 'lcc', 'PN6121..PN6129@4', 0),
    ('unified:c20fed72-8f7c-5b34-b370-39f778be1b4f', 'lcc', 'PN6146.2..PN6231@4', 0),
    ('bokheim:subject:literature:genres', 'lcc', 'PN6120.15..PN6120.95@4', 2);

INSERT OR IGNORE INTO source_label(concept_id, label, ordinal) VALUES
    ('unified:7f34a373-09e9-5e61-a9f2-69a11350098d', 'Essays', 1),
    ('unified:0359f62a-0aa4-5708-aa76-05ec604b0bca', 'Letters', 0),
    ('unified:85dd3fd2-15b0-58a3-b2c6-27e36e218196', 'Quotations', 0),
    ('unified:84143bb3-fe56-5ec4-a2b3-5e4b6a3fe45d', 'Drama', 2),
    ('unified:373583c3-8d1b-5a8f-b52c-c6414ea34808', 'Poetry', 1),
    ('unified:f359eef0-cb25-55ac-ba46-8b3ac8e4a40c', 'Orations', 0),
    ('unified:c20fed72-8f7c-5b34-b370-39f778be1b4f', 'Wit and humor', 0),
    ('bokheim:subject:literature:genres', 'Fiction', 0);

DELETE FROM concept
WHERE concept_id IN (
    'bokheim:lcc-detail:bc0f269f-b79d-509d-96f9-f2d1673a22a8',
    'bokheim:lcc-detail:a7fe0a71-62c6-5e29-88dd-2059fa13ca37',
    'unified:82412c16-157e-51c5-8347-2fd9773936e5',
    'unified:97dd3c50-1551-5fe3-bc19-3234c58fd30b',
    'bokheim:lcc-detail:15cfa4fe-87a8-50cb-8828-33591ce97c2e',
    'bokheim:lcc-detail:f5e58ee6-5ef5-5396-9d89-2ae0b664fddb',
    'bokheim:lcc-detail:7b68cbbd-b071-583d-b52f-09822513dda3',
    'bokheim:lcc-detail:5e79a739-cd62-598b-a7bf-e0bb5afd51d8',
    'bokheim:lcc-detail:c14b2728-dd1e-5069-a85f-37db9fb3c84b',
    'bokheim:lcc-detail:1b6549de-5ca1-5dab-b080-f8e2ff798d3d'
);

-- Move the remaining distinct LCC collection forms beneath the unified
-- Literary Collections concept.
UPDATE concept_parent
SET parent_concept_id = 'unified:aeec07de-7d62-59b5-90c5-f775cf9f888c'
WHERE parent_concept_id = 'unified:ed1cc769-f28a-59df-b83a-4e2b81540b37';

DELETE FROM concept
WHERE concept_id = 'unified:ed1cc769-f28a-59df-b83a-4e2b81540b37';

DELETE FROM concept
WHERE concept_id = 'bokheim:subject:literature:general:collections-personal';

COMMIT;

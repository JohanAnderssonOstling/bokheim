-- Reconcile both Economics & Business industry branches into the existing
-- concise Industries concept while preserving BISAC and LCC coverage.
-- This migration is intentionally idempotent and follows the earlier
-- Economics reconciliations from the same date.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

INSERT OR IGNORE INTO source_selector(concept_id, system_id, selector, ordinal) VALUES
    ('unified:9ccac193-2002-5554-ab6f-076977b5f112', 'lcc', 'HD', 0),
    ('unified:9ccac193-2002-5554-ab6f-076977b5f112', 'lcc', 'HD28..HD9999@1', 1);

DELETE FROM concept_parent
WHERE parent_concept_id IN (
    'bokheim:subject:economics:industries',
    'unified:e1e1a593-e41e-5a0d-9243-65711bec3cec'
);

INSERT OR IGNORE INTO concept_parent(concept_id, parent_concept_id, ordinal, display_order) VALUES
    ('unified:9ccac193-2002-5554-ab6f-076977b5f112', 'unified:103d333e-4f6d-5d1c-9c4b-950677c8d681', 0, 10),

    ('unified:2cde3e6c-02ff-53a3-8ae2-f8b74c0594fa', 'unified:9ccac193-2002-5554-ab6f-076977b5f112', 0, 20),
    ('unified:2189c9e7-f114-56ac-b0ee-3033f00723fd', 'unified:9ccac193-2002-5554-ab6f-076977b5f112', 0, 21),
    ('unified:605f273f-ef51-5d4e-8455-f62396db0ccb', 'unified:9ccac193-2002-5554-ab6f-076977b5f112', 0, 22),
    ('unified:650a72d9-5ca1-5a13-980f-c75472263560', 'unified:9ccac193-2002-5554-ab6f-076977b5f112', 0, 23),

    ('unified:c92f48be-7525-56da-9f95-9c4b98aedce1', 'unified:9ccac193-2002-5554-ab6f-076977b5f112', 0, 30),
    ('unified:8e51c653-486b-59e9-b73c-a63e78a420fc', 'unified:9ccac193-2002-5554-ab6f-076977b5f112', 0, 31),
    ('unified:6cd36c31-3f37-5ae2-af25-b27360c7b523', 'unified:9ccac193-2002-5554-ab6f-076977b5f112', 0, 32),
    ('unified:ff3e273a-ef37-56e3-b3e9-bae12b75ebab', 'unified:9ccac193-2002-5554-ab6f-076977b5f112', 0, 33),
    ('unified:abc1f0a6-b2c0-586e-8714-dbe9042848fa', 'unified:9ccac193-2002-5554-ab6f-076977b5f112', 0, 34),
    ('unified:d5c35eeb-7ec5-5d9b-b24c-6f1e80251e3c', 'unified:9ccac193-2002-5554-ab6f-076977b5f112', 0, 35);

DELETE FROM concept
WHERE concept_id IN (
    'bokheim:subject:economics:industries',
    'unified:e1e1a593-e41e-5a0d-9243-65711bec3cec'
);

COMMIT;

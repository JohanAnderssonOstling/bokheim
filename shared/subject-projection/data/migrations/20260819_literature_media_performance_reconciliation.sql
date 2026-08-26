-- Reconcile the legacy Literature > Media & Performance LCC concepts with the
-- existing Arts & Media concepts. This migration is intentionally idempotent.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

-- Broadcasting spans radio and television in LCC, so preserve each range on
-- its corresponding unified performing-arts concept.
INSERT OR IGNORE INTO source_selector(concept_id, system_id, selector, ordinal) VALUES
    ('unified:f49e8c68-5000-55fd-beb4-60b3519e09c2', 'lcc', 'PN1991..PN1991.9@4', 0),
    ('unified:4bc8ddaf-719e-53d7-88de-35175860f07f', 'lcc', 'PN1992..PN1992.92@4', 0),
    ('unified:3a5d2554-58b3-548b-8c98-54f02e7a0e87', 'lcc', 'PN1997..PN1997.85@4', 0);

DELETE FROM concept
WHERE concept_id IN (
    'unified:551b3864-ffbd-5738-af40-51dd94731dce',
    'unified:6b5b3129-c508-59e7-bd61-bc3eebc83629'
);

DELETE FROM concept
WHERE concept_id = 'bokheim:subject:literature:general:media';

COMMIT;

-- Apply approved items 4-5 from the fourth Social Sciences duplicate review.
-- Human Sexuality and the other reviewed candidates remain unchanged.
-- This migration is intentionally idempotent.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

-- Distinguish the law-specific category from business Corporate Finance.
UPDATE concept
SET preferred_label = 'Corporate Finance Law'
WHERE concept_id = 'bokheim:lcc-detail:16cebd1a-8de5-5178-a1cb-8d429e2f0fca';

-- Merge the detailed LCC New Business Enterprises entry into the BISAC-backed
-- category beneath Entrepreneurship.
INSERT OR IGNORE INTO source_selector(concept_id, system_id, selector, ordinal) VALUES
    ('unified:66fcc9c8-e6b0-56d7-a00d-2b8d39cfa4fb', 'lcc', 'HG4027.6..HG4027.6@4', 0);

INSERT OR IGNORE INTO source_label(concept_id, label, ordinal) VALUES
    ('unified:66fcc9c8-e6b0-56d7-a00d-2b8d39cfa4fb', 'New business enterprises', 0);

DELETE FROM concept
WHERE concept_id = 'bokheim:lcc-detail:a8a7e6d8-f781-52f0-a2a6-56b77f055668';

COMMIT;

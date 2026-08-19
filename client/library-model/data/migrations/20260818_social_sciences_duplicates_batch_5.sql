-- Reconcile cross-system duplicate concepts reviewed in the fifth Social
-- Sciences batch, and distinguish the two meanings of Interest.
-- This migration is intentionally idempotent.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

-- Behaviorism: retain the curated entry beneath Psychology > Movements.
INSERT OR IGNORE INTO source_selector(concept_id, system_id, selector, ordinal) VALUES
    ('unified:d4dbefac-a423-50a0-8237-b4c098ca9846', 'lcc', 'BF199..BF199@2', 0);

INSERT OR IGNORE INTO source_label(concept_id, label, ordinal) VALUES
    ('unified:d4dbefac-a423-50a0-8237-b4c098ca9846', 'Behaviorism. Neobehaviorism. Behavioral psychology', 0);

DELETE FROM concept
WHERE concept_id = 'bokheim:philosophy-lcc-full:4ab60950-98d1-5c6e-bb45-991790a1c6cf';

-- Business Etiquette: retain the curated Governance & Ethics entry.
INSERT OR IGNORE INTO source_selector(concept_id, system_id, selector, ordinal) VALUES
    ('unified:57e909f9-a639-5cca-9ad9-4b5b8e0ff99b', 'lcc', 'HF5389..HF5389.3@4', 0);

INSERT OR IGNORE INTO source_label(concept_id, label, ordinal) VALUES
    ('unified:57e909f9-a639-5cca-9ad9-4b5b8e0ff99b', 'Business etiquette. Office etiquette', 0);

DELETE FROM concept
WHERE concept_id = 'bokheim:business:etiquette';

-- Business Mathematics: retain the curated Business Skills & Reference entry.
INSERT OR IGNORE INTO source_selector(concept_id, system_id, selector, ordinal) VALUES
    ('unified:b68e0249-6e60-59ca-bcfd-7caa71559ad2', 'lcc', 'HF5691..HF5716@4', 0);

INSERT OR IGNORE INTO source_label(concept_id, label, ordinal) VALUES
    ('unified:b68e0249-6e60-59ca-bcfd-7caa71559ad2', 'Business mathematics. Commercial arithmetic', 0);

DELETE FROM concept
WHERE concept_id = 'bokheim:business:mathematics';

-- Foreign Exchange: retain the curated direct Finance entry.
INSERT OR IGNORE INTO source_selector(concept_id, system_id, selector, ordinal) VALUES
    ('unified:45bcabe6-1129-54e8-9852-cb132c3d4892', 'lcc', 'HG3810..HG3877.2@4', 0);

INSERT OR IGNORE INTO source_label(concept_id, label, ordinal) VALUES
    ('unified:45bcabe6-1129-54e8-9852-cb132c3d4892', 'Foreign exchange', 0);

DELETE FROM concept
WHERE concept_id = 'bokheim:lcc-detail:ab327376-9e6a-5d27-ba52-3fb5d9b268c7';

-- Interest: keep the applied-finance and economic-theory concepts separate.
UPDATE concept
SET preferred_label = 'Interest Rates'
WHERE concept_id = 'unified:c1329268-e817-5303-ae5e-fbaff3709107';

UPDATE concept
SET preferred_label = 'Interest Theory'
WHERE concept_id = 'bokheim:lcc-detail:0f8196c2-6b19-5196-8067-f8e3f61de75e';

COMMIT;

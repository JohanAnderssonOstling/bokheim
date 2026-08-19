-- Remove exact preferred-label collisions between sibling concepts. These are
-- rejected by the runtime taxonomy installer. Concepts with detailed children
-- are retained, and source coverage is preserved. This migration is idempotent.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

-- Church of England outside of Great Britain.
INSERT OR IGNORE INTO source_selector(concept_id, system_id, selector, ordinal) VALUES
    ('unified:c4b40660-f7f8-541e-9033-13154ff2a691', 'lcc', 'BX5600..BX5743.52@4', 2);

INSERT OR IGNORE INTO source_label(concept_id, label, ordinal) VALUES
    ('unified:c4b40660-f7f8-541e-9033-13154ff2a691', 'Church of England outside of Great Britain', 0);

DELETE FROM concept
WHERE concept_id = 'bokheim:lcc-detail:6cfcb940-b614-5946-a783-aa5c4ebea6c8';

-- Medieval Empire, 481-1273.
INSERT OR IGNORE INTO source_selector(concept_id, system_id, selector, ordinal) VALUES
    ('unified:f82ead2b-4193-5bcc-821e-25335cea5abe', 'lcc', 'DD126..DD155@6', 4);

DELETE FROM concept
WHERE concept_id = 'bokheim:history-lcc:27277274-00e2-5c1a-8008-ce7defd7ba73';

-- Tunisia (Tunis).
INSERT OR IGNORE INTO source_selector(concept_id, system_id, selector, ordinal) VALUES
    ('unified:12cbc983-844a-56a4-a344-dbb1de18d7ef', 'lcc', 'DT241..DT269@5', 0);

INSERT OR IGNORE INTO source_label(concept_id, label, ordinal) VALUES
    ('unified:12cbc983-844a-56a4-a344-dbb1de18d7ef', 'Tunisia (Tunis)', 1);

DELETE FROM concept
WHERE concept_id = 'bokheim:history-lcc-full:b660514e-9644-5ee9-8ae3-3336587a2d36';

-- Congo (Democratic Republic). Zaire. Belgian Congo.
DELETE FROM concept
WHERE concept_id = 'bokheim:history-lcc:6c61d771-20cd-556d-b398-0893e7151eb0';

COMMIT;

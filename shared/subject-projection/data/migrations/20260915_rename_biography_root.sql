-- Use the shorter root label while retaining the existing concept identity,
-- BISAC BIO000000 selector, and child assignments.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

UPDATE concept
SET preferred_label='Biography'
WHERE concept_id=3212;

COMMIT;

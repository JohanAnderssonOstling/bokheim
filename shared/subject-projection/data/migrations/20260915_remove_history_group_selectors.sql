-- History grouping nodes have no direct current code; child ranges carry the
-- actual country-level classification.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

DELETE FROM lcc_selector
WHERE concept_id IN (5540,3385,12963);

COMMIT;

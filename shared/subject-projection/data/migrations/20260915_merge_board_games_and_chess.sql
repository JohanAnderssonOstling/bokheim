-- Consolidate the duplicate BISAC/LCC game concepts.
-- Board Games 1505 is the existing Games & Activities subject. Chess 6047 is
-- the existing LCC subject; retain it and add the BISAC Chess selector.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

INSERT OR IGNORE INTO bisac_selector (concept_id, selector)
SELECT 1505, selector
FROM bisac_selector
WHERE concept_id=4631;

INSERT OR IGNORE INTO bisac_selector (concept_id, selector)
SELECT 6047, selector
FROM bisac_selector
WHERE concept_id=4636;

DELETE FROM concept_parent WHERE concept_id IN (4630,4636,4631,6047);

INSERT INTO concept_parent (concept_id, parent_concept_id, ordinal)
VALUES (4630,1505,0), (6047,1505,1);

DELETE FROM bisac_selector WHERE concept_id IN (4631,4636);
DELETE FROM lcc_selector WHERE concept_id=4636;
DELETE FROM ddc_selector WHERE concept_id IN (4631,4636);
DELETE FROM source_label WHERE concept_id IN (4631,4636);
DELETE FROM concept WHERE concept_id IN (4631,4636);

COMMIT;

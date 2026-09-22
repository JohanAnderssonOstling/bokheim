-- Merge the residual general-history leaf into the Italian Ancient Era.
-- These ranges describe ancient Italian history generally, rather than a
-- distinct subject that needs its own child node.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

INSERT OR IGNORE INTO lcc_range(start_code,end_code,concept_id)
SELECT start_code,end_code,3571
FROM lcc_range
WHERE concept_id=13627;

DELETE FROM lcc_range WHERE concept_id=13627;
DELETE FROM bisac_selector WHERE concept_id=13627;
DELETE FROM source_label WHERE concept_id=13627;
DELETE FROM concept_parent WHERE concept_id=13627;
DELETE FROM concept WHERE concept_id=13627;

COMMIT;

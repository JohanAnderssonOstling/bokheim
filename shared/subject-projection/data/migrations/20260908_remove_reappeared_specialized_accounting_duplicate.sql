PRAGMA foreign_keys=ON;
BEGIN;
INSERT OR IGNORE INTO lcc_selector(concept_id,selector) SELECT 52584,selector FROM lcc_selector WHERE concept_id=53490;
INSERT OR IGNORE INTO bisac_selector(concept_id,selector) SELECT 52584,selector FROM bisac_selector WHERE concept_id=53490;
INSERT OR IGNORE INTO ddc_selector(selector,concept_id) SELECT selector,52584 FROM ddc_selector WHERE concept_id=53490;
UPDATE lcc_range SET concept_id=52584 WHERE concept_id=53490;
DELETE FROM concept_parent WHERE concept_id=53490 OR parent_concept_id=53490;
DELETE FROM concept WHERE concept_id=53490;
COMMIT;

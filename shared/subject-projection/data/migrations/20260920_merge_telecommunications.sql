-- Combine the LCC Telecommunication branch with BISAC Telecommunications.
-- Keep Telecommunications (453) and retain every classification and child.
PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;
INSERT OR IGNORE INTO lcc_selector SELECT 453,selector FROM lcc_selector WHERE concept_id=263;
INSERT OR IGNORE INTO bisac_selector SELECT 453,selector FROM bisac_selector WHERE concept_id=263;
UPDATE lcc_range SET concept_id=453 WHERE concept_id=263;
UPDATE ddc_selector SET concept_id=453 WHERE concept_id=263;
DELETE FROM concept_parent AS old
WHERE old.parent_concept_id=263
  AND EXISTS(SELECT 1 FROM concept_parent kept WHERE kept.concept_id=old.concept_id AND kept.parent_concept_id=453);
UPDATE concept_parent SET parent_concept_id=453 WHERE parent_concept_id=263;
DELETE FROM concept_parent WHERE concept_id=263;
DELETE FROM concept WHERE concept_id=263;
COMMIT;

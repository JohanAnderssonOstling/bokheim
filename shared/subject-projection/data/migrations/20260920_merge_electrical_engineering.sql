-- Unify the BISAC Electrical node and broad LCC Electrical Engineering node
-- under Electrical & Electronic Engineering, preserving their classifications.
PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;
INSERT OR IGNORE INTO lcc_selector SELECT 5566,selector FROM lcc_selector WHERE concept_id IN (249,250);
INSERT OR IGNORE INTO bisac_selector SELECT 5566,selector FROM bisac_selector WHERE concept_id IN (249,250);
UPDATE lcc_range SET concept_id=5566 WHERE concept_id IN (249,250);
UPDATE ddc_selector SET concept_id=5566 WHERE concept_id IN (249,250);
-- Radar already has the surviving parent. Keep that edge and its ordinal.
DELETE FROM concept_parent AS old
WHERE old.parent_concept_id IN (249,250)
  AND EXISTS(SELECT 1 FROM concept_parent kept WHERE kept.concept_id=old.concept_id AND kept.parent_concept_id=5566);
UPDATE concept_parent SET parent_concept_id=5566 WHERE parent_concept_id IN (249,250);
DELETE FROM concept_parent WHERE concept_id IN (249,250);
DELETE FROM concept WHERE concept_id IN (249,250);
COMMIT;

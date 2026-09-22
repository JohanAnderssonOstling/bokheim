BEGIN;

CREATE TEMP TABLE removed_state_detail(concept_id INTEGER PRIMARY KEY) WITHOUT ROWID;
INSERT INTO removed_state_detail VALUES
  -- North Carolina periods and low-hit western region.
  (51204),(51205),(51206),(51207),(51208),
  -- South Carolina periods.
  (51292),(51293),(51294),(51295),
  -- Tennessee period.
  (51306),
  -- Colorado periods.
  (51313),(51314),
  -- Alaska periods.
  (51319),(51320),(51321),(51322),(51323),
  -- Low-hit cities.
  (51228),(51309),(51333),(51335);

DELETE FROM concept_parent WHERE concept_id IN (SELECT concept_id FROM removed_state_detail)
  OR parent_concept_id IN (SELECT concept_id FROM removed_state_detail);
DELETE FROM lcc_range WHERE concept_id IN (SELECT concept_id FROM removed_state_detail);
DELETE FROM lcc_selector WHERE concept_id IN (SELECT concept_id FROM removed_state_detail);
DELETE FROM bisac_selector WHERE concept_id IN (SELECT concept_id FROM removed_state_detail);
DELETE FROM source_label WHERE concept_id IN (SELECT concept_id FROM removed_state_detail);
DELETE FROM concept WHERE concept_id IN (SELECT concept_id FROM removed_state_detail);

-- Consolidate chronological and otherwise removed regional spans directly on
-- their state parent. Washington, D.C. periods intentionally remain intact.
INSERT OR IGNORE INTO lcc_range(class_letters,start_value,end_value,concept_id) VALUES
  ('F','257','261.999',3462),
  ('F','272','275.499',3469),
  ('F','440','440.299',3471),
  ('F','780','781.999',3436),
  ('F','907','910.799',3432);

DROP TABLE removed_state_detail;

COMMIT;

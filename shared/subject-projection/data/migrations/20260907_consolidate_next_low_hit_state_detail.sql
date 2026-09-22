BEGIN;

CREATE TEMP TABLE removed_state_detail(concept_id INTEGER PRIMARY KEY) WITHOUT ROWID;
INSERT INTO removed_state_detail VALUES
  (51230),                         -- Michigan Since 1951
  (51212),(51213),(51214),(51215), -- all Georgia periods
  (51197),(51227),(51334);         -- Annapolis, Columbus, Newark

DELETE FROM concept_parent WHERE concept_id IN (SELECT concept_id FROM removed_state_detail)
  OR parent_concept_id IN (SELECT concept_id FROM removed_state_detail);
DELETE FROM lcc_range WHERE concept_id IN (SELECT concept_id FROM removed_state_detail);
DELETE FROM lcc_selector WHERE concept_id IN (SELECT concept_id FROM removed_state_detail);
DELETE FROM bisac_selector WHERE concept_id IN (SELECT concept_id FROM removed_state_detail);
DELETE FROM source_label WHERE concept_id IN (SELECT concept_id FROM removed_state_detail);
DELETE FROM concept WHERE concept_id IN (SELECT concept_id FROM removed_state_detail);

-- Fold the removed chronological spans into the state parents. The removed
-- city spans are already contained by their Cities & Towns parent ranges.
INSERT OR IGNORE INTO lcc_range(class_letters,start_value,end_value,concept_id) VALUES
  ('F','570','570.299',3451),
  ('F','289','291.399',3440);

DROP TABLE removed_state_detail;

COMMIT;

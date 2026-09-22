BEGIN;

-- These period splits are too fine for the curated state trees. Return their
-- LCC spans to the state leaves and remove the navigational concepts.
CREATE TEMP TABLE removed_state_detail(concept_id INTEGER PRIMARY KEY) WITHOUT ROWID;
INSERT INTO removed_state_detail VALUES
  (51191),(51192),(51193),(51194), -- Maryland periods
  (51220),(51221),(51222),         -- Ohio periods
  (51327),(51328),(51329),(51330), -- New Jersey periods
  (51199),(51203),(51211),(51219),(51229),(51234),
  (51291),(51299),(51305),(51312),(51318),(51326),(51336);

DELETE FROM concept_parent WHERE concept_id IN (SELECT concept_id FROM removed_state_detail)
  OR parent_concept_id IN (SELECT concept_id FROM removed_state_detail);
DELETE FROM lcc_range WHERE concept_id IN (SELECT concept_id FROM removed_state_detail);
DELETE FROM lcc_selector WHERE concept_id IN (SELECT concept_id FROM removed_state_detail);
DELETE FROM bisac_selector WHERE concept_id IN (SELECT concept_id FROM removed_state_detail);
DELETE FROM source_label WHERE concept_id IN (SELECT concept_id FROM removed_state_detail);
DELETE FROM concept WHERE concept_id IN (SELECT concept_id FROM removed_state_detail);

INSERT OR IGNORE INTO lcc_range(class_letters,start_value,end_value,concept_id) VALUES
  ('F','184','186.399',3449),
  ('F','495','496.499',3464),
  ('F','137','140.299',3459);

-- Population history is sparse state-level detail. Keep the classifications
-- useful by folding each span back into its state parent.
INSERT OR IGNORE INTO lcc_range(class_letters,start_value,end_value,concept_id) VALUES
  ('F','190.A1','190.Z',3449),
  ('F','235.A1','235.Z',3475),
  ('F','265.A1','265.Z',3462),
  ('F','295.A1','295.Z',3440),
  ('F','500.A1','500.Z',3464),
  ('F','575.A1','575.Z',3451),
  ('F','205.A1','205.Z',50911),
  ('F','280.A1','280.Z',3469),
  ('F','335.A1','335.Z',3431),
  ('F','445.A1','445.Z',3471),
  ('F','785.A1','785.Z',3436),
  ('F','915.A1','915.Z',3432),
  ('F','145.A1','145.Z',3459);

DROP TABLE removed_state_detail;

COMMIT;

-- Complete the QC schedule partition. Mathematical physics, force and energy,
-- and weights and measures already have their own ranges; cover only the
-- occupied residual portions of the general QC1-QC75 division.

INSERT INTO concept(concept_id,preferred_label)
VALUES (53351,'Physics Reference, History & Study');

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal)
VALUES (53351,5030,0);

DELETE FROM lcc_range
WHERE concept_id=5030
  AND start_code='QC1'
  AND end_code='QC999';

INSERT INTO lcc_range(start_code,end_code,concept_id)
VALUES
  ('QC1','QC19.1',53351),
  ('QC20.86','QC71.81',53351),
  ('QC73.81','QC75',53351);

INSERT OR IGNORE INTO lcc_selector(concept_id,selector)
VALUES
  (5030,'QC1-75'),
  (5030,'QC1-999');

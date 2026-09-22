-- Partition the two official geophysics divisions around their existing exact
-- and ranged specialists. Printed whole divisions remain exact on the parent.

INSERT INTO concept(concept_id,preferred_label)
VALUES
  (53349,'Geophysics Reference & Study'),
  (53350,'Geomagnetism Reference & Study');

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal)
VALUES
  (53349,5036,0),
  (53350,5036,0);

DELETE FROM lcc_range WHERE concept_id=5036;

INSERT INTO lcc_range(start_code,end_code,concept_id)
VALUES
  ('QC801','QC801.2',53349),
  ('QC801.4','QC801.9',53349),
  ('QC806','QC806.9',53349),
  ('QC807.5','QC807.6',53349),
  ('QC809','QC809.Z',53349),
  ('QC811','QC817.Z',53350),
  ('QC819','QC824.Z',53350);

INSERT OR IGNORE INTO lcc_selector(concept_id,selector)
VALUES
  (5036,'QC801-809'),
  (5036,'QC811-849');

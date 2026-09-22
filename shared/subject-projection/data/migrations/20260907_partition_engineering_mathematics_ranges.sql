-- Partition TA329-TA348 around the existing numerical, graphical, statistical,
-- modeling, and computer-aided engineering leaves.

INSERT INTO concept(concept_id,preferred_label)
VALUES
  (53370,'Engineering Mathematics Reference & Study'),
  (53371,'Engineering Mathematical Aids');

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal)
VALUES
  (53370,285,0),
  (53371,285,0);

DELETE FROM lcc_range WHERE concept_id=285;

INSERT INTO lcc_range(start_code,end_code,concept_id)
VALUES
  ('TA329','TA334.9',53370),
  ('TA346','TA348',53371);

INSERT OR IGNORE INTO lcc_selector(concept_id,selector)
VALUES (285,'TA329-348');

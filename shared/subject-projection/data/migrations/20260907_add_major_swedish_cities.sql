BEGIN;

INSERT INTO concept(concept_id,preferred_label) VALUES
  (51400,'Gothenburg'),
  (51401,'Uppsala'),
  (51402,'Malmö');

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal) VALUES
  (51400,13750,0),(51401,13750,0),(51402,13750,0);

INSERT INTO lcc_range(class_letters,start_value,end_value,concept_id) VALUES
  ('DL','991.G6','991.G695',51400),
  ('DL','991.U7','991.U79',51401),
  ('DL','991.M2','991.M29',51402);

COMMIT;

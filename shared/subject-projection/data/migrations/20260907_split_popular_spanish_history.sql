BEGIN;

INSERT INTO concept(concept_id,preferred_label) VALUES
  (51501,'Asturias'),(51502,'Majorca'),(51503,'Francisco Franco'),
  (51504,'Granada'),(51505,'Murcia'),(51506,'Córdoba'),
  (51507,'Extremadura'),(51508,'Castilla y León'),
  (51509,'Juan Carlos I'),(51510,'Málaga');

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal) VALUES
  (51501,13847,0),(51502,12753,0),
  (51503,10220,0),(51503,13353,1),
  (51504,13847,0),(51505,13847,0),(51506,13847,0),
  (51507,13847,0),(51508,13847,0),
  (51509,10205,0),(51509,13353,1),(51510,13847,0);

INSERT INTO lcc_range(class_letters,start_value,end_value,concept_id) VALUES
  ('DP','302.O8','302.O89',51501),('DP','302.B27','302.B279',51502),
  ('DP','264.F7','264.F79',51503),('DP','402.G6','402.G69',51504),
  ('DP','302.M8','302.M89',51505),('DP','402.C7','402.C79',51506),
  ('DP','302.E83','302.E839',51507),('DP','302.C556','302.C5569',51508),
  ('DP','272.4.J8','272.4.J89',51509),('DP','402.M2','402.M29',51510);

COMMIT;

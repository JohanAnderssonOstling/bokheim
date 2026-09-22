BEGIN;

UPDATE concept SET preferred_label='Italian Regions & Cities' WHERE concept_id=13851;

INSERT INTO concept(concept_id,preferred_label) VALUES
  (51490,'Benito Mussolini'),(51491,'Giuseppe Garibaldi'),
  (51492,'Aldo Moro'),(51493,'Silvio Berlusconi'),
  (51494,'Trentino-Alto Adige'),(51495,'Apulia'),(51496,'Veneto'),
  (51497,'Umbria'),(51498,'Trieste'),(51499,'Sardinian History'),
  (51500,'Siena');

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal) VALUES
  (51490,8430,0),(51490,13353,1),
  (51491,6307,0),(51491,13353,1),
  (51492,9133,0),(51492,13353,1),
  (51493,8279,0),(51493,13353,1),
  (51494,13851,0),(51495,13851,0),(51496,13851,0),
  (51497,13851,0),(51498,13851,0),(51499,13851,0),(51500,13851,0);

INSERT INTO lcc_range(class_letters,start_value,end_value,concept_id) VALUES
  ('DG','575.M8','575.M89',51490),('DG','552.8.G2','552.8.G29',51491),
  ('DG','579.M63','579.M639',51492),('DG','583.B47','583.B479',51493),
  ('DG','975.T792','975.T7929',51494),('DG','975.A65','975.A659',51495),
  ('DG','975.V38','975.V389',51496),('DG','975.U5','975.U59',51497),
  ('DG','975.T825','975.T8259',51498),('DG','975.S31','975.S319',51499),
  ('DG','975.S5','975.S59',51500);

COMMIT;

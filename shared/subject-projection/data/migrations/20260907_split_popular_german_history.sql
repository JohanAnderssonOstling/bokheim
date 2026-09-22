BEGIN;

INSERT INTO concept(concept_id,preferred_label) VALUES
  (51477,'Adolf Hitler'),(51478,'Resistance to Nazism'),
  (51479,'Political History since Reunification'),
  (51480,'Foreign Relations since Reunification'),
  (51481,'Turkish Germans'),(51482,'Ludwig II of Bavaria'),
  (51483,'Berlin, 1945–1990'),(51484,'Berlin Social & Cultural History'),
  (51485,'Mainz'),(51486,'Münster'),(51487,'Potsdam'),
  (51488,'Regensburg'),(51489,'Weimar');

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal) VALUES
  (51477,3559,0),(51477,13353,1),(51478,3559,0),
  (51479,13706,0),(51480,13706,0),(51480,51029,1),
  (51481,13705,0),(51482,11447,0),(51482,13353,1),
  (51483,11448,0),(51484,11448,0),
  (51485,13904,0),(51486,13904,0),(51487,13904,0),
  (51488,13904,0),(51489,13904,0);

INSERT INTO lcc_range(class_letters,start_value,end_value,concept_id) VALUES
  ('DD','247.H5','247.H59',51477),('DD','256.3','256.399',51478),
  ('DD','290.29','290.2999',51479),('DD','290.3','290.399',51480),
  ('DD','78.T87','78.T879',51481),('DD','801.B387','801.B3879',51482),
  ('DD','881','881.999',51483),('DD','866','866.999',51484),
  ('DD','901.M21','901.M219',51485),('DD','901.M696','901.M6969',51486),
  ('DD','901.P8','901.P89',51487),('DD','901.R4','901.R49',51488),
  ('DD','901.W4','901.W49',51489);

INSERT OR IGNORE INTO lcc_selector(concept_id,selector) VALUES (2460,'DD859');

COMMIT;

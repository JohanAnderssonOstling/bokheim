BEGIN;

INSERT INTO concept(concept_id,preferred_label) VALUES
  (51167,'African American History, 1863–1877'),
  (51168,'African American History, 1877–1964'),
  (51169,'African American History Since 1964'),
  (51170,'African American Interracial Relations'),
  (51171,'African American Identity'),
  (51172,'African American Military History'),
  (51173,'African American Religion'),
  (51174,'African American Economic Conditions'),
  (51175,'African American Social Life & Customs'),
  (51176,'Other African American Topics');

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal) VALUES
  (51167,3378,0),(51168,3378,0),(51169,3378,0),
  (51170,3378,0),(51171,3378,0),
  (51172,3378,0),(51172,9765,1),
  (51173,3378,0),(51174,3378,0),(51175,3378,0),(51176,3378,0);

DELETE FROM lcc_range WHERE concept_id=50850;

INSERT INTO lcc_range(class_letters,start_value,end_value,concept_id) VALUES
  ('E','185.2','185.499',51167),
  ('E','185.5','185.6149',51168),
  ('E','185.615','185.6199',51169),
  ('E','185.62','185.6249',51170),
  ('E','185.625','185.6299',51171),
  ('E','185.63','185.6999',51172),
  ('E','185.7','185.7999',51173),
  ('E','185.8','185.8599',51174),
  ('E','185.86','185.8899',51175),
  ('E','185.89','185.8999',51176);

-- The former wrapper would have no selectors after the split and adds no
-- useful navigation level, so remove it rather than retain an artificial node.
DELETE FROM concept WHERE concept_id=50850;

COMMIT;

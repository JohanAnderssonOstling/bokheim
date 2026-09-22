BEGIN;

INSERT INTO concept(concept_id,preferred_label) VALUES
  (50986,'Early Virginia to 1775'),
  (50987,'1775–1865'),
  (50988,'1865–1950'),
  (50989,'1951–');

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal) VALUES
  (50986,3475,0),(50987,3475,0),(50988,3475,0),(50989,3475,0);

-- Official LCC Virginia chronology. F231.3 is biography and F231.9 is
-- Southwest Virginia, so those remain with Virginia rather than an era.
INSERT INTO lcc_range(class_letters,start_value,end_value,concept_id) VALUES
  ('F','229','229.999',50986),
  ('F','230','230.999',50987),
  ('F','231','231.199',50988),
  ('F','231.2','231.299',50989);

COMMIT;

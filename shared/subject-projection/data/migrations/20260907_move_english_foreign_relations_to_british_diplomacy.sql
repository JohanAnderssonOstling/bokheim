BEGIN;

-- Keep permissive ranges on leaves while presenting them through the shared
-- British diplomatic-history branch.
INSERT INTO concept(concept_id,preferred_label) VALUES
  (51452,'British Foreign Relations by Country'),
  (51453,'British Diplomatic History, 20th Century');

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal) VALUES
  (51452,51030,0),
  (51453,51030,0),(51453,10115,1);

INSERT INTO lcc_range(class_letters,start_value,end_value,concept_id) VALUES
  ('DA','47','47.999',51452),
  ('DA','566.7','566.799',51453),
  ('DA','589.8','589.899',51453);

COMMIT;

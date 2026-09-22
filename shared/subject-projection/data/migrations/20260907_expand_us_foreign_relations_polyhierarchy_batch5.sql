BEGIN;

INSERT INTO concept(concept_id,preferred_label) VALUES
  (51076,'Afghan Foreign Relations History'),
  (51077,'Argentine Foreign Relations History'),
  (51078,'Egyptian Foreign Relations History'),
  (51079,'U.S.–Afghanistan Relations'),
  (51080,'U.S.–Argentina Relations'),
  (51081,'U.S.–Egypt Relations');

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal) VALUES
  (51076,3486,0),(51077,10499,0),(51078,3635,0),
  (51079,51022,0),(51079,51076,1),
  (51080,51020,0),(51080,51077,1),
  (51081,51023,0),(51081,51078,1);

INSERT INTO lcc_selector(concept_id,selector) VALUES
  (51079,'E183.8.A3'),
  (51080,'E183.8.A7'),
  (51081,'E183.8.E35');

COMMIT;

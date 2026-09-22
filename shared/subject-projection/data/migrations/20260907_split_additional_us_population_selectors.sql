BEGIN;

INSERT INTO concept(concept_id,preferred_label) VALUES
  (50877,'Chinese American History'),
  (50878,'Italian American History'),
  (50879,'German American History'),
  (50880,'Irish American History'),
  (50881,'Japanese American History'),
  (50882,'Asian American History'),
  (50883,'Korean American History');

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal) VALUES
  (50877,13702,0),
  (50878,13702,0),
  (50879,13702,0),
  (50880,13702,0),
  (50881,13702,0),
  (50882,13702,0),
  (50883,13702,0);

-- These are individual population Cutters, not permissive ranges. Adjacent
-- Cutter values describe unrelated communities and must remain independent.
INSERT INTO source_selector(concept_id,system_id,selector) VALUES
  (50877,'lcc','E184.C5'),
  (50878,'lcc','E184.I8'),
  (50879,'lcc','E184.G3'),
  (50880,'lcc','E184.I6'),
  (50881,'lcc','E184.J3'),
  (50882,'lcc','E184.A75'),
  (50883,'lcc','E184.K6');

COMMIT;

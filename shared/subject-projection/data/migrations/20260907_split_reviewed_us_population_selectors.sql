BEGIN;

INSERT INTO concept(concept_id,preferred_label) VALUES
  (50874,'Mexican American History'),
  (50875,'Spanish American History'),
  (50876,'Jewish American History');

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal) VALUES
  (50874,13702,0),
  (50875,13702,0),
  (50876,13702,0);

-- Exact cutters only: adjacent E184 cutters represent unrelated population
-- groups, so none of these may be widened into a permissive range.
INSERT INTO source_selector(concept_id,system_id,selector) VALUES
  (50874,'lcc','E184.M5'),
  (50875,'lcc','E184.S75'),
  (50876,'lcc','E184.J5');

COMMIT;

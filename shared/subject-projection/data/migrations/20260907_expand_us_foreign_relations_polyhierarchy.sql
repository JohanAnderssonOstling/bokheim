BEGIN;

INSERT INTO concept(concept_id,preferred_label) VALUES
  (51044,'Israeli Foreign Relations History'),
  (51045,'Mexican Foreign Relations History'),
  (51046,'Indian Foreign Relations History'),
  (51047,'United States–Israel Relations'),
  (51048,'United States–Mexico Relations'),
  (51049,'United States–India Relations');

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal) VALUES
  (51044,3638,0),(51045,3391,0),(51046,3496,0),
  (51047,51023,0),(51047,51044,1),
  (51048,51020,0),(51048,51045,1),
  (51049,51022,0),(51049,51046,1);

INSERT INTO source_selector(concept_id,system_id,selector) VALUES
  (51047,'lcc','E183.8.I7'),
  (51048,'lcc','E183.8.M6'),
  (51049,'lcc','E183.8.I4'),
  (51041,'lcc','E183.8.K7');

COMMIT;

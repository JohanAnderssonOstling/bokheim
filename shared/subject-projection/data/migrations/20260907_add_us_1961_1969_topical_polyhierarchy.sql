BEGIN;

INSERT INTO concept(concept_id,preferred_label) VALUES
  (51001,'U.S. Biography'),
  (51002,'By Period'),
  (51003,'By Period'),
  (51004,'By Period'),
  (51005,'U.S. Political History, 1961–1969'),
  (51006,'U.S. Military History, 1961–1969'),
  (51007,'U.S. Biography, 1961–1969');

-- Topical browsing routes.
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal) VALUES
  (51001,3402,0),
  (51002,3416,0),
  (51003,9765,0),
  (51004,51001,0);

-- Each leaf is shared by its chronological and topical routes.
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal) VALUES
  (51005,50990,0),(51005,51002,1),
  (51006,50990,0),(51006,51003,1),
  (51007,50990,0),(51007,51004,1);

INSERT INTO lcc_range(class_letters,start_value,end_value,concept_id) VALUES
  ('E','839.5','839.899',51005),
  ('E','840.4','840.599',51006),
  ('E','840.6','840.899',51007);

COMMIT;

BEGIN;

INSERT INTO concept(concept_id,preferred_label) VALUES
  (51186,'U.S. Political History, 20th Century'),
  (51187,'U.S. Foreign Relations, 20th Century'),
  (51188,'U.S. Military History, 20th Century'),
  (51189,'U.S. Naval History, 20th Century'),
  (51190,'U.S. Biography, 20th Century');

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal) VALUES
  (51186,3405,0),(51186,51002,1),
  (51187,3405,0),(51187,3410,1),
  (51188,3405,0),(51188,51003,1),
  (51189,3405,0),(51189,51003,1),
  (51190,3405,0),(51190,51004,1);

DELETE FROM lcc_range
WHERE concept_id=3405 AND class_letters='E'
  AND start_value='740' AND end_value='749';

-- General collections and general works remain on the chronological parent.
INSERT INTO lcc_range(class_letters,start_value,end_value,concept_id) VALUES
  ('E','740','742.999',3405),
  ('E','749','749.999',3405),
  ('E','743','743.999',51186),
  ('E','744','744.999',51187),
  ('E','745','745.999',51188),
  ('E','746','746.999',51189),
  ('E','747','748.999',51190);

COMMIT;

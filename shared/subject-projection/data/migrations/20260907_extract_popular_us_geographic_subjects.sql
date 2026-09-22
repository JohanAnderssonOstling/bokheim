BEGIN;

-- Once their major named regions are extracted, these umbrella labels no
-- longer need to duplicate the child names.
UPDATE concept SET preferred_label='Rocky Mountains' WHERE concept_id=50908;
UPDATE concept SET preferred_label='New Southwest' WHERE concept_id=50909;

INSERT INTO concept(concept_id,preferred_label) VALUES
  (51363,'Colorado River & Canyon'),
  (51364,'Las Vegas'),
  (51365,'Yellowstone National Park'),
  (51366,'Yosemite National Park'),
  (51367,'Hudson River Valley'),
  (51368,'Adirondack Mountains'),
  (51369,'Statue of Liberty'),
  (51370,'Savannah'),
  (51371,'Southern Appalachians'),
  (51372,'Chesapeake Bay Region');

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal) VALUES
  (51363,50909,0),(51363,3436,1),
  (51364,3457,0),
  (51365,50908,0),
  (51366,50952,0),
  (51367,50956,0),(51368,50956,0),
  (51369,50921,0),
  (51370,51217,0),
  (51371,3422,0),
  (51372,51195,0);

-- Leaf ranges include cataloguing suffixes while remaining narrower than the
-- surrounding regional ranges, so every notation resolves to one subject.
INSERT INTO lcc_range(class_letters,start_value,end_value,concept_id) VALUES
  ('F','788','788.999',51363),
  ('F','849.L35','849.L359',51364),
  ('F','722','722.999',51365),
  ('F','868.Y6','868.Y69',51366),
  ('F','127.H8','127.H89',51367),
  ('F','127.A2','127.A29',51368),
  ('F','128.64.L6','128.64.L69',51369),
  ('F','294.S2','294.S29',51370),
  ('F','217.A65','217.A659',51371),
  ('F','187.C5','187.C59',51372),
  ('F','591','591.999',3478);

COMMIT;

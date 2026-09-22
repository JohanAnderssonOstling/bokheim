BEGIN;

INSERT OR IGNORE INTO concept(concept_id,preferred_label) VALUES
  (51381,'Lewis and Clark Expedition'),
  (51382,'San Francisco Bay Region'),
  (51383,'White House'),
  (51384,'Pacific Northwest'),
  (51385,'Seattle'),
  (51386,'Williamsburg'),
  (51387,'Long Island'),
  (51388,'Jamestown'),
  (51389,'Sierra Nevada'),
  (51390,'Great Smoky Mountains'),
  (51391,'Death Valley');

INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal) VALUES
  (51381,3402,0),
  (51382,50952,0),
  (51383,51290,0),
  (51384,3423,0),
  (51385,3476,0),
  (51386,51201,0),
  (51387,50956,0),
  (51388,51201,0),
  (51389,50952,0),
  (51390,51307,0),
  (51391,50952,0);

-- This exact Seattle span previously sat directly on Washington.
DELETE FROM lcc_range
WHERE class_letters='F' AND start_value='899.S4' AND end_value='899.S49';

INSERT OR IGNORE INTO lcc_range(class_letters,start_value,end_value,concept_id) VALUES
  ('F','592.7','592.799',51381),
  ('F','868.S156','868.S1569',51382),
  ('F','204.W5','204.W59',51383),
  ('F','597','597.999',51384),
  ('F','899.S4','899.S49',51385),
  ('F','234.W7','234.W79',51386),
  ('F','127.L8','127.L89',51387),
  ('F','234.J3','234.J39',51388),
  ('F','868.S5','868.S59',51389),
  ('F','443.G7','443.G79',51390),
  ('F','868.D2','868.D29',51391);

COMMIT;

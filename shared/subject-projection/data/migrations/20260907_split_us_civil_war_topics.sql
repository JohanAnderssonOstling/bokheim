BEGIN;

INSERT INTO concept(concept_id,preferred_label) VALUES
  (51086,'Civil War Biography'),
  (51087,'Civil War General Histories'),
  (51088,'Civil War Diplomatic History'),
  (51089,'Civil War Military Operations'),
  (51090,'Civil War Finance & Commerce'),
  (51091,'Confederate States of America'),
  (51092,'Civil War Armies & Troops'),
  (51093,'Civil War Naval History'),
  (51094,'Civil War Personal Narratives'),
  (51095,'Civil War Prisoners & Prisons'),
  (51096,'Civil War Medical & Hospital Services'),
  (51097,'Civil War Relief & Charities'),
  (51098,'Civil War Memorials & Monuments'),
  (51099,'Civil War Illustrative Material');

-- Chronological access from the Civil War period. Biography and the three
-- military subjects also appear in their respective U.S. topical-by-period
-- branches, preserving the established polyhierarchy.
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal) VALUES
  (51086,3408,0),(51086,51004,1),
  (51087,3408,0),
  (51088,3408,0),
  (51089,3408,0),(51089,51003,1),
  (51090,3408,0),
  (51091,3408,0),
  (51092,3408,0),(51092,51003,1),
  (51093,3408,0),(51093,51003,1),
  (51094,3408,0),
  (51095,3408,0),
  (51096,3408,0),
  (51097,3408,0),
  (51098,3408,0),
  (51099,3408,0);

-- Replace the overlapping E456-E655 umbrellas with mutually exclusive LCC
-- section ranges. E456-E463.999 is the broad period material and remains on
-- the parent. The detailed battle/campaign numbers inside Military Operations
-- are deliberately kept flattened into that broad leaf.
DELETE FROM lcc_range WHERE concept_id=3408 AND class_letters='E';

INSERT INTO lcc_range(class_letters,start_value,end_value,concept_id) VALUES
  ('E','456','463.999',3408),
  ('E','464','467.999',51086),
  ('E','468','468.999',51087),
  ('E','469','469.999',51088),
  ('E','470','478.999',51089),
  ('E','480','480.999',51090),
  ('E','482','489.999',51091),
  ('E','491','590.999',51092),
  ('E','591','600.999',51093),
  ('E','601','610.999',51094),
  ('E','611','620.999',51095),
  ('E','621','628.999',51096),
  ('E','629','634.999',51097),
  ('E','641','646.499',51098),
  ('E','647','655.999',51099);

COMMIT;

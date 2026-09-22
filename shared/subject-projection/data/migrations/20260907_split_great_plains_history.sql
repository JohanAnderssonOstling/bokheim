BEGIN;

INSERT INTO concept(concept_id,preferred_label) VALUES
  (50912,'Great Plains Regional History'),
  (50913,'Early History to 1848'),
  (50914,'1848–1860'),
  (50915,'1860–1880'),
  (50916,'1880–1950'),
  (50917,'1951–1980'),
  (50918,'1981–'),
  (50919,'Frontier, Pioneer & Ranch Life'),
  (50920,'Population Groups of the Great Plains');

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal) VALUES
  (50912,50907,0),
  (50913,50907,0),
  (50914,50907,0),
  (50915,50907,0),
  (50916,50907,0),
  (50917,50907,0),
  (50918,50907,0),
  (50919,50907,0),
  (50920,50907,0);

-- Replace the broad Great Plains umbrella with disjoint leaf ranges.
DELETE FROM lcc_range
WHERE concept_id=50907 AND class_letters='F'
  AND start_value='590.2' AND end_value='596';

INSERT INTO lcc_range(class_letters,start_value,end_value,concept_id) VALUES
  ('F','590.2','591.999',50912),
  ('F','592','592.999',50913),
  ('F','593','593.999',50914),
  ('F','594','594.999',50915),
  ('F','595','595.199999',50916),
  ('F','595.2','595.299999',50917),
  ('F','595.3','595.999',50918),
  ('F','596','596.199999',50919);

UPDATE lcc_range SET concept_id=50920
WHERE concept_id=50907 AND class_letters='F'
  AND start_value='596.2' AND end_value='596.3';

COMMIT;

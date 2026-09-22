BEGIN;

INSERT INTO concept(concept_id,preferred_label) VALUES
  (50976,'1821–1865'),
  (50977,'1865–1950'),
  (50978,'1951–');

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal) VALUES
  (50976,3439,0),(50977,3439,0),(50978,3439,0);

-- Reuse the existing early-Florida child and include decimal subdivisions.
DELETE FROM source_selector
WHERE concept_id=50818 AND system_id='lcc' AND selector='F314';

INSERT INTO lcc_range(class_letters,start_value,end_value,concept_id) VALUES
  ('F','314','314.999',50818),
  ('F','315','315.999',50976),
  ('F','316','316.199999',50977),
  ('F','316.2','316.999',50978);

COMMIT;

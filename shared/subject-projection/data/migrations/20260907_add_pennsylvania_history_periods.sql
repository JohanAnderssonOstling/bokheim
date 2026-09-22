BEGIN;

INSERT INTO concept(concept_id,preferred_label) VALUES
  (50983,'Colonial Pennsylvania to 1775'),
  (50984,'1775–1865'),
  (50985,'1865–');

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal) VALUES
  (50983,3467,0),(50984,3467,0),(50985,3467,0);

-- The official LCC Pennsylvania chronology is F152-F154. Keep the wider
-- F146-F160 umbrella on Pennsylvania for topical and locality residuals;
-- longest-range specificity routes these three blocks to their era leaves.
INSERT INTO lcc_range(class_letters,start_value,end_value,concept_id) VALUES
  ('F','152','152.999',50983),
  ('F','153','153.999',50984),
  ('F','154','154.999',50985);

COMMIT;

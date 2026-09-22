-- Complete the TA401-TA492 Engineering Materials partition.

INSERT INTO concept(concept_id,preferred_label)
VALUES
  (53372,'Engineering Materials Reference & Testing'),
  (53373,'Materials of Special Composition & Structure');

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal)
VALUES
  (53372,289,0),
  (53373,7707,0);

DELETE FROM lcc_range WHERE concept_id IN (289,7153,6625);

INSERT INTO lcc_range(start_code,end_code,concept_id)
VALUES
  ('TA401','TA404.79',53372),
  ('TA404.8','TA418.49',7153),
  ('TA418.5','TA418.84',6625),
  ('TA418.9.A','TA418.9.Z',53373);

INSERT OR IGNORE INTO lcc_selector(concept_id,selector)
VALUES
  (289,'TA401-492'),
  (7153,'TA404.8-418.84'),
  (7153,'TA404.6'),
  (6625,'TA418.5-418.84'),
  (409,'TA418.9.N35');

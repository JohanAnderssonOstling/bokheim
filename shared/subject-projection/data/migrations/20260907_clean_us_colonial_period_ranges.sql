BEGIN;

INSERT INTO concept(concept_id,preferred_label) VALUES
  (51337,'Colonial America, 1607–1689'),
  (51338,'Colonial America, 1689–1775');

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal) VALUES
  (51337,3409,0),(51338,3409,0);

DELETE FROM lcc_range WHERE concept_id=3409 AND class_letters='E';

-- General colonial works and societies remain on the parent. The two broad
-- chronological leaves absorb the official war subdivisions without exposing
-- individual wars, campaigns, or battles as taxonomy subjects.
INSERT INTO lcc_range(class_letters,start_value,end_value,concept_id) VALUES
  ('E','186','189.999',3409),
  ('E','201','202.999',3409),
  ('E','191','194.999',51337),
  ('E','195','199.2',51338);

COMMIT;

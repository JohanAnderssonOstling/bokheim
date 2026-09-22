BEGIN;

INSERT INTO concept(concept_id,preferred_label) VALUES
  (51119,'Revolutionary Era Biography'),
  (51120,'Revolutionary Political History'),
  (51121,'Declaration of Independence'),
  (51122,'Revolutionary War Military Operations'),
  (51123,'Revolutionary Diplomatic History'),
  (51124,'Revolutionary Armies & Troops'),
  (51125,'Revolutionary Naval History'),
  (51126,'Revolutionary Personal Narratives'),
  (51127,'Women in the American Revolution'),
  (51128,'Loyalists'),
  (51129,'Revolutionary War Espionage'),
  (51130,'Revolutionary War Prisoners'),
  (51131,'Revolutionary War Medical Services'),
  (51132,'Revolutionary Commemoration & Cultural Material');

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal) VALUES
  (51119,3424,0),(51119,51004,1),
  (51120,3424,0),(51120,51002,1),
  (51121,3424,0),
  (51122,3424,0),(51122,51003,1),
  (51123,3424,0),
  (51124,3424,0),(51124,51003,1),
  (51125,3424,0),(51125,51003,1),
  (51126,3424,0),
  (51127,3424,0),
  (51128,3424,0),
  (51129,3424,0),
  (51130,3424,0),
  (51131,3424,0),
  (51132,3424,0);

-- E201-E202 belongs to the end of the colonial schedule, not the Revolution.
-- Reuse the existing Colonial Period concept for that material.
INSERT INTO lcc_range(class_letters,start_value,end_value,concept_id)
VALUES ('E','201','202.999',3409);

-- Remove the Revolution's overlapping umbrella and internal duplicate ranges.
-- Broad collections and general histories remain directly on the parent.
DELETE FROM lcc_range WHERE concept_id=3424 AND class_letters='E';

INSERT INTO lcc_range(class_letters,start_value,end_value,concept_id) VALUES
  ('E','203','205.999',3424),
  ('E','208','209.999',3424),
  ('E','206','207.999',51119),
  ('E','210','216.999',51120),
  ('E','221','221.999',51121),
  ('E','230','241.999',51122),
  ('E','249','250.999',51123),
  ('E','251','270.999',51124),
  ('E','271','274.999',51125),
  ('E','275','275.999',51126),
  ('E','276','276.999',51127),
  ('E','277','278.999',51128),
  ('E','279','280.999',51129),
  ('E','281','282.999',51130),
  ('E','283','284.999',51131),
  ('E','285','298.999',51132);

COMMIT;

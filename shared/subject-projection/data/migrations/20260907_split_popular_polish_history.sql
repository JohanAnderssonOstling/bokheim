BEGIN;

INSERT INTO concept(concept_id,preferred_label) VALUES
  (51537,'Polish Social & Cultural History'),
  (51538,'Polish National Identity'),(51539,'Polish Diaspora'),
  (51540,'Germans in Poland'),(51541,'Ukrainians in Poland'),
  (51542,'Polish Foreign Relations by Country');

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal) VALUES
  (51537,3610,0),(51538,3610,0),(51539,3610,0),
  (51540,3610,0),(51541,3610,0),(51542,3610,0);

INSERT INTO lcc_range(class_letters,start_value,end_value,concept_id) VALUES
  ('DK','4110','4119.999',51537),('DK','4121','4121.499',51538),
  ('DK','4122','4122.999',51539),('DK','4121.5.G4','4121.5.G49',51540),
  ('DK','4121.5.U4','4121.5.U49',51541),
  ('DK','4185.A','4185.Z',51542);

COMMIT;

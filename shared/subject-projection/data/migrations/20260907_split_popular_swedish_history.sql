BEGIN;

INSERT INTO concept(concept_id,preferred_label) VALUES
  (51392,'Gustav V, 1907–1950'),
  (51393,'Gustav VI Adolf, 1950–1973'),
  (51394,'Carl XVI Gustaf, 1973–'),
  (51395,'Stockholm'),
  (51396,'Swedish Lapland'),
  (51397,'Sámi History & Culture'),
  (51398,'Scania'),
  (51399,'Gotland');

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal) VALUES
  (51392,11557,0),(51393,11557,0),(51394,11557,0),
  (51395,13750,0),
  (51396,13749,0),(51398,13749,0),(51399,13749,0),
  (51397,13934,0);

-- The modern periods follow the official Swedish DL schedule. Stockholm is
-- limited to its numeric subdivision; alphabetic DL976 cutters remain with
-- the general Swedish cities node.
INSERT INTO lcc_range(class_letters,start_value,end_value,concept_id) VALUES
  ('DL','867','870.Z',51392),
  ('DL','872','876.Z',51393),
  ('DL','877','879.Z',51394),
  ('DL','976','976.399',51395),
  ('DL','971.L2','971.L29',51396),
  ('DL','641.L35','641.L359',51397),
  ('DL','971.S3','971.S39',51398),
  ('DL','971.G7','971.G79',51399);

COMMIT;

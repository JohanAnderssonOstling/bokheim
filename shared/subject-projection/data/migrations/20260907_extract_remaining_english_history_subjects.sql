BEGIN;

INSERT INTO concept(concept_id,preferred_label) VALUES
  (51424,'Horatio Nelson'),
  (51425,'Hadrian’s Wall'),
  (51426,'South Asians in England'),
  (51427,'East End of London'),
  (51428,'Gloucestershire'),
  (51429,'Muslims in England'),
  (51430,'Thomas More'),
  (51431,'Benjamin Disraeli');

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal) VALUES
  (51424,50862,0),(51424,13353,1),
  (51425,10093,0),(51425,13834,1),
  (51426,50861,0),(51427,13835,0),(51428,13658,0),
  (51429,50861,0),
  (51430,10149,0),(51430,13353,1),
  (51431,10213,0),(51431,13353,1);

INSERT INTO lcc_range(class_letters,start_value,end_value,concept_id) VALUES
  ('DA','87.1.N4','87.1.N49',51424),
  ('DA','146','146.999',51425),
  ('DA','125.S57','125.S579',51426),
  ('DA','685.E1','685.E19',51427),
  ('DA','670.G5','670.G59',51428),
  ('DA','125.M87','125.M879',51429),
  ('DA','334.M8','334.M89',51430),
  ('DA','564.B3','564.B39',51431);

-- DA679 is explicitly London guidebooks. An exact selector on British Travel
-- overrides the surrounding London-history range without making that
-- non-leaf travel node permissive.
INSERT OR IGNORE INTO lcc_selector(concept_id,selector) VALUES (2461,'DA679');

COMMIT;

BEGIN;

INSERT INTO concept(concept_id,preferred_label) VALUES
  (51058,'Africa'),
  (51059,'Nicaraguan Foreign Relations History'),
  (51060,'South African Foreign Relations History'),
  (51061,'Spanish Foreign Relations History'),
  (51062,'Italian Foreign Relations History'),
  (51063,'U.S.–Nicaragua Relations'),
  (51064,'U.S.–South Africa Relations'),
  (51065,'U.S.–Spain Relations'),
  (51066,'U.S.–Italy Relations');

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal) VALUES
  (51058,3410,4),
  (51059,3392,0),(51060,3367,0),(51061,3613,0),(51062,3607,0),
  (51063,51020,0),(51063,51059,1),
  (51064,51058,0),(51064,51060,1),
  (51065,51021,0),(51065,51061,1),
  (51066,51021,0),(51066,51062,1);

INSERT INTO source_selector(concept_id,system_id,selector) VALUES
  (51063,'lcc','E183.8.N5'),
  (51064,'lcc','E183.8.S6'),
  (51065,'lcc','E183.8.S7'),
  (51066,'lcc','E183.8.I8');

COMMIT;

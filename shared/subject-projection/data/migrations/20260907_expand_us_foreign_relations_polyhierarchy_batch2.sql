BEGIN;

INSERT INTO concept(concept_id,preferred_label) VALUES
  (51050,'Turkish Foreign Relations History'),
  (51051,'Iraqi Foreign Relations History'),
  (51052,'Vietnamese Foreign Relations History'),
  (51053,'Taiwanese Foreign Relations History'),
  (51054,'U.S.–Turkey Relations'),
  (51055,'U.S.–Iraq Relations'),
  (51056,'U.S.–Vietnam Relations'),
  (51057,'U.S.–Taiwan Relations');

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal) VALUES
  (51050,3551,0),(51051,3637,0),(51052,3513,0),(51053,10699,0),
  (51054,51023,0),(51054,51050,1),
  (51055,51023,0),(51055,51051,1),
  (51056,51022,0),(51056,51052,1),
  (51057,51022,0),(51057,51053,1);

INSERT INTO source_selector(concept_id,system_id,selector) VALUES
  (51054,'lcc','E183.8.T8'),
  (51055,'lcc','E183.8.I57'),
  (51056,'lcc','E183.8.V5'),
  (51057,'lcc','E183.8.T3');

COMMIT;

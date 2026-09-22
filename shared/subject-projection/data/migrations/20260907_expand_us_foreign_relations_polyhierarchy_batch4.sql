BEGIN;

INSERT INTO concept(concept_id,preferred_label) VALUES
  (51067,'Oceania'),
  (51068,'Panamanian Foreign Relations History'),
  (51069,'Australian Foreign Relations History'),
  (51070,'Brazilian Foreign Relations History'),
  (51071,'Pakistani Foreign Relations History'),
  (51072,'U.S.–Panama Relations'),
  (51073,'U.S.–Australia Relations'),
  (51074,'U.S.–Brazil Relations'),
  (51075,'U.S.–Pakistan Relations');

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal) VALUES
  (51067,3410,5),
  (51068,3393,0),(51069,3677,0),(51070,3427,0),(51071,3505,0),
  (51072,51020,0),(51072,51068,1),
  (51073,51067,0),(51073,51069,1),
  (51074,51020,0),(51074,51070,1),
  (51075,51022,0),(51075,51071,1);

INSERT INTO lcc_selector(concept_id,selector) VALUES
  (51072,'E183.8.P2'),
  (51073,'E183.8.A8'),
  (51074,'E183.8.B7'),
  (51075,'E183.8.P18');

COMMIT;

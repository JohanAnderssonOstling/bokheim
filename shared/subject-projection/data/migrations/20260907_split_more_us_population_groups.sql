BEGIN;

INSERT INTO concept(concept_id,preferred_label) VALUES
  (51155,'Polish American History'),
  (51156,'Cuban American History'),
  (51157,'Russian American History'),
  (51158,'Vietnamese American History'),
  (51159,'Arab American History'),
  (51160,'Indian American History'),
  (51161,'Filipino American History'),
  (51162,'Muslim American History'),
  (51163,'Norwegian American History'),
  (51164,'Mennonite & Amish American History'),
  (51165,'Swedish American History'),
  (51166,'Greek American History');

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal) VALUES
  (51155,13702,0),(51156,13702,0),(51157,13702,0),
  (51158,13702,0),(51159,13702,0),(51160,13702,0),
  (51161,13702,0),(51162,13702,0),(51163,13702,0),
  (51164,13702,0),(51165,13702,0),(51166,13702,0);

INSERT INTO lcc_selector(concept_id,selector) VALUES
  (51155,'E184.P7'),
  (51156,'E184.C97'),
  (51157,'E184.R9'),
  (51158,'E184.V53'),
  (51159,'E184.A65'),
  (51160,'E184.E2'),
  (51161,'E184.F4'),
  (51162,'E184.M88'),
  (51163,'E184.S2'),
  (51164,'E184.M45'),
  (51165,'E184.S23'),
  (51166,'E184.G7');

-- The obsolete LCC caption "Orientals" is folded into the existing Asian
-- American History subject rather than exposed as a taxonomy label.
INSERT INTO lcc_selector(concept_id,selector) VALUES (50882,'E184.O6');

COMMIT;

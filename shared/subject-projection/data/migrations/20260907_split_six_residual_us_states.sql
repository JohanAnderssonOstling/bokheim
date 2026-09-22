BEGIN;

UPDATE concept SET preferred_label='Georgia' WHERE concept_id=3440;

INSERT INTO concept(concept_id,preferred_label) VALUES
  (51191,'Early Maryland to 1775'),(51192,'1775–1865'),(51193,'1865–1950'),(51194,'1951–'),
  (51195,'Maryland Regions & Counties'),(51196,'Maryland Cities & Towns'),
  (51197,'Annapolis'),(51198,'Baltimore'),(51199,'Maryland Population History'),

  (51200,'Virginia Regions & Counties'),(51201,'Virginia Cities & Towns'),
  (51202,'Richmond'),(51203,'Virginia Population History'),

  (51204,'Early North Carolina to 1775'),(51205,'1775–1865'),
  (51206,'1865–1950'),(51207,'1951–'),(51208,'Western North Carolina'),
  (51209,'North Carolina Regions & Counties'),(51210,'North Carolina Cities & Towns'),
  (51211,'North Carolina Population History'),

  (51212,'Early Georgia to 1775'),(51213,'1775–1865'),
  (51214,'1865–1950'),(51215,'1951–'),
  (51216,'Georgia Regions & Counties'),(51217,'Georgia Cities & Towns'),
  (51218,'Atlanta'),(51219,'Georgia Population History'),

  (51220,'Ohio to 1865'),(51221,'1865–1950'),(51222,'1951–'),
  (51223,'Ohio Regions & Counties'),(51224,'Ohio Cities & Towns'),
  (51225,'Cincinnati'),(51226,'Cleveland'),(51227,'Columbus'),(51228,'Dayton'),
  (51229,'Ohio Population History'),

  (51230,'Michigan Since 1951'),(51231,'Michigan Regions & Counties'),
  (51232,'Michigan Cities & Towns'),(51233,'Detroit'),
  (51234,'Michigan Population History');

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal) VALUES
  (51191,3449,0),(51192,3449,0),(51193,3449,0),(51194,3449,0),
  (51195,3449,0),(51196,3449,0),(51197,51196,0),(51198,51196,0),(51199,3449,0),
  (51200,3475,0),(51201,3475,0),(51202,51201,0),(51203,3475,0),
  (51204,3462,0),(51205,3462,0),(51206,3462,0),(51207,3462,0),
  (51208,3462,0),(51209,3462,0),(51210,3462,0),(51211,3462,0),
  (51212,3440,0),(51213,3440,0),(51214,3440,0),(51215,3440,0),
  (51216,3440,0),(51217,3440,0),(51218,51217,0),(51219,3440,0),
  (51220,3464,0),(51221,3464,0),(51222,3464,0),(51223,3464,0),(51224,3464,0),
  (51225,51224,0),(51226,51224,0),(51227,51224,0),(51228,51224,0),(51229,3464,0),
  (51230,3451,0),(51231,3451,0),(51232,3451,0),(51233,51232,0),(51234,3451,0);

-- Remove each state's umbrella and redundant nested ranges. Broad reference,
-- biography, monuments, antiquities, and undifferentiated history stay on the
-- state parent; official chronological and geographic spans move to children.
DELETE FROM lcc_range WHERE concept_id IN (3449,3475,3462,3440,3464,3451);

INSERT INTO lcc_range(class_letters,start_value,end_value,concept_id) VALUES
  ('F','176','183.999',3449),
  ('F','184','184.999',51191),('F','185','185.999',51192),
  ('F','186','186.199',51193),('F','186.2','186.399',51194),
  ('F','187.A','187.Z',51195),('F','189.A','189.Z',51196),
  ('F','189.A6','189.A69',51197),('F','189.B1','189.B19',51198),
  ('F','190.A1','190.Z',51199),

  ('F','221','228.999',3475),
  ('F','232.A','232.Z',51200),('F','234.A','234.Z',51201),
  ('F','234.R5','234.R59',51202),('F','235.A1','235.Z',51203),

  ('F','251','256.999',3462),
  ('F','257','257.999',51204),('F','258','258.999',51205),
  ('F','259','259.999',51206),('F','260','260.499',51207),
  ('F','261','261.999',51208),('F','262.A','262.Z',51209),
  ('F','264.A','264.Z',51210),('F','265.A1','265.Z',51211),

  ('F','281','288.999',3440),
  ('F','289','289.999',51212),('F','290','290.999',51213),
  ('F','291','291.199',51214),('F','291.2','291.399',51215),
  ('F','292.A','292.Z',51216),('F','294.A','294.Z',51217),
  ('F','294.A8','294.A89',51218),('F','295.A1','295.Z',51219),

  ('F','486','494.999',3464),
  ('F','495','495.999',51220),('F','496','496.199',51221),
  ('F','496.2','496.499',51222),('F','497.A','497.Z',51223),
  ('F','499.A','499.Z',51224),('F','499.C5','499.C59',51225),
  ('F','499.C6','499.C69',51226),('F','499.C7','499.C79',51227),
  ('F','499.D2','499.D29',51228),('F','500.A1','500.Z',51229),

  ('F','561','569.999',3451),('F','570','570.299',51230),
  ('F','572.A','572.Z',51231),('F','574.A','574.Z',51232),
  ('F','574.D4','574.D49',51233),('F','575.A1','575.Z',51234);

-- Exact guidebook numbers override the broad state-parent ranges and resolve
-- to travel without requiring artificial state-travel navigation nodes.
INSERT INTO lcc_selector(concept_id,selector) VALUES
  (2521,'F179.3'),(2521,'F224.3'),(2521,'F252.3'),
  (2521,'F284.3'),(2521,'F489.3'),(2521,'F564.3');

COMMIT;

PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;

UPDATE concept SET preferred_label='Political Institutions & Public Administration — General'
WHERE concept_id='unified:d559fba0-8aae-58dd-9df0-7e459aef7cb2';

DELETE FROM source_selector
WHERE concept_id='unified:d559fba0-8aae-58dd-9df0-7e459aef7cb2';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) VALUES
('unified:d559fba0-8aae-58dd-9df0-7e459aef7cb2','lcc','JF',0),
('unified:d559fba0-8aae-58dd-9df0-7e459aef7cb2','lcc','JF20..JF2112@1',1);

CREATE TEMP TABLE gov_split(
  concept_id TEXT PRIMARY KEY,
  label TEXT NOT NULL,
  selector TEXT NOT NULL UNIQUE,
  display_order INTEGER NOT NULL
) WITHOUT ROWID;

INSERT INTO gov_split VALUES
('bokheim:politics:government:north-america-general','North American Government — General','JJ1000..JJ1019@2',500),
('bokheim:politics:government:americas-general','Government of the Americas — General','JL',510),
('bokheim:politics:government:canada','Canadian Government & Administration','JL1..JL500@2',520),
('bokheim:politics:government:caribbean','Caribbean Government & Administration','JL599.5..JL839@2',530),
('bokheim:politics:government:latin-america-general','Latin American Government — General','JL840..JL1199.99@2',540),
('bokheim:politics:government:mexico','Mexican Government & Administration','JL1200..JL1299@2',550),
('bokheim:politics:government:central-america','Central American Government & Administration','JL1400..JL1679@2',560),
('bokheim:politics:government:south-america','South American Government & Administration','JL1850..JL3899@2',570);

INSERT INTO concept(concept_id,preferred_label)
SELECT concept_id,label FROM gov_split;
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal,display_order)
SELECT concept_id,'bokheim:politics:government-administration',0,display_order
FROM gov_split;
INSERT INTO source_selector(concept_id,system_id,selector,ordinal)
SELECT concept_id,'lcc',selector,0 FROM gov_split;

COMMIT;

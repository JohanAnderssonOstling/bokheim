PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;

CREATE TEMP TABLE politics_split(
  concept_id TEXT PRIMARY KEY,
  label TEXT NOT NULL,
  parent_id TEXT NOT NULL,
  selector TEXT NOT NULL UNIQUE,
  display_order INTEGER NOT NULL
) WITHOUT ROWID;

INSERT INTO politics_split VALUES
('bokheim:politics:ideology:communism-socialism','Communism, Post-Communism & Socialism','bokheim:politics:theory-ideologies','POL005000',10),
('bokheim:politics:ideology:democracy','Democracy','bokheim:politics:theory-ideologies','POL007000',20),
('bokheim:politics:ideology:nationalism-patriotism','Nationalism & Patriotism','bokheim:politics:theory-ideologies','POL031000',30),
('bokheim:politics:ideology:anarchism','Anarchism','bokheim:politics:theory-ideologies','POL042010',40),
('bokheim:politics:ideology:conservatism-liberalism','Conservatism & Liberalism','bokheim:politics:theory-ideologies','POL042020',50),
('bokheim:politics:ideology:fascism-totalitarianism','Fascism & Totalitarianism','bokheim:politics:theory-ideologies','POL042030',60),
('bokheim:politics:ideology:radicalism','Radicalism','bokheim:politics:theory-ideologies','POL042040',70),
('bokheim:politics:ideology:libertarianism','Libertarianism','bokheim:politics:theory-ideologies','POL042050',80),
('bokheim:politics:ideology:capitalism','Capitalism','bokheim:politics:theory-ideologies','POL042060',90),

('bokheim:politics:process:campaigns-elections','Campaigns & Elections','bokheim:politics:process-media','POL008000',10),
('bokheim:politics:process:political-parties','Political Parties','bokheim:politics:process-media','POL015000',20),
('bokheim:politics:process:political-advocacy','Political Advocacy','bokheim:politics:process-media','POL043000',30),
('bokheim:politics:process:media-internet','Political Media & Internet','bokheim:politics:process-media','POL065000',40),

('bokheim:politics:policy:urban-development','City Planning & Urban Development','bokheim:politics:policy-economy','POL002000',10),
('bokheim:politics:policy:welfare-services','Social Services & Welfare Policy','bokheim:politics:policy-economy','POL019000',20),
('bokheim:politics:policy:regional-planning','Regional Planning','bokheim:politics:policy-economy','POL026000',30),
('bokheim:politics:policy:social-security','Social Security Policy','bokheim:politics:policy-economy','POL027000',40),
('bokheim:politics:policy:social','Social Policy','bokheim:politics:policy-economy','POL029000',50),
('bokheim:politics:policy:cultural','Cultural Policy','bokheim:politics:policy-economy','POL038000',60),
('bokheim:politics:policy:environmental','Environmental Policy','bokheim:politics:policy-economy','POL044000',70),
('bokheim:politics:policy:communications','Communication Policy','bokheim:politics:policy-economy','POL050000',80),
('bokheim:politics:policy:science-technology','Science & Technology Policy','bokheim:politics:policy-economy','POL063000',90),
('bokheim:politics:policy:agriculture-food','Agriculture & Food Policy','bokheim:politics:policy-economy','POL067000',100),
('bokheim:politics:policy:energy','Energy Policy','bokheim:politics:policy-economy','POL068000',110),
('bokheim:politics:policy:military','Military Policy','bokheim:politics:policy-economy','POL069000',120),
('bokheim:politics:policy:immigration','Immigration Policy','bokheim:politics:policy-economy','POL070000',130),
('bokheim:politics:policy:health-care','Health-Care Policy','bokheim:politics:policy-economy','POL073000',140),

('bokheim:politics:world:africa','African Politics','bokheim:politics:international-global','POL053000',10),
('bokheim:politics:world:asia','Asian Politics','bokheim:politics:international-global','POL054000',20),
('bokheim:politics:world:australia-oceania','Australian & Oceanian Politics','bokheim:politics:international-global','POL055000',30),
('bokheim:politics:world:canada','Canadian Politics','bokheim:politics:international-global','POL056000',40),
('bokheim:politics:world:caribbean-latin-america','Caribbean & Latin American Politics','bokheim:politics:international-global','POL057000',50),
('bokheim:politics:world:europe','European Politics','bokheim:politics:international-global','POL058000',60),
('bokheim:politics:world:middle-east','Middle Eastern Politics','bokheim:politics:international-global','POL059000',70),
('bokheim:politics:world:russia-former-soviet','Russian & Former Soviet Politics','bokheim:politics:international-global','POL060000',80);

INSERT INTO concept(concept_id,preferred_label)
SELECT concept_id,label FROM politics_split;

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal,display_order)
SELECT concept_id,parent_id,0,display_order FROM politics_split;

INSERT INTO source_selector(concept_id,system_id,selector,ordinal)
SELECT concept_id,'bisac',selector,0 FROM politics_split;

DELETE FROM source_selector
WHERE concept_id IN (
 'unified:7a37ab00-fe08-5589-bda2-9df765b2e984',
 'unified:fa59b522-3346-5628-9333-06ee6c7b01ae',
 'unified:40b5c498-0b47-5d4b-88d4-d1f15c11a199',
 'unified:f09dacee-3420-5624-96fd-8a95f1a7e452'
)
AND selector IN (SELECT selector FROM politics_split);

UPDATE concept SET preferred_label='Political Ideologies — General'
WHERE concept_id='unified:7a37ab00-fe08-5589-bda2-9df765b2e984';
UPDATE concept SET preferred_label='Political Process — General'
WHERE concept_id='unified:fa59b522-3346-5628-9333-06ee6c7b01ae';
UPDATE concept SET preferred_label='Public Policy — General'
WHERE concept_id='unified:40b5c498-0b47-5d4b-88d4-d1f15c11a199';
UPDATE concept SET preferred_label='World Politics — General'
WHERE concept_id='unified:f09dacee-3420-5624-96fd-8a95f1a7e452';

DROP TABLE politics_split;
COMMIT;

PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;

-- JQ spans Asia, Africa, Australasia, and the Pacific. The imported concept
-- called the whole subclass "Asian" and repeated dozens of nested ranges.
UPDATE concept SET preferred_label='Asian Government — General'
WHERE concept_id='unified:a09fe73e-8aad-594a-a17f-682b961daad4';
DELETE FROM source_selector
WHERE concept_id='unified:a09fe73e-8aad-594a-a17f-682b961daad4';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) VALUES
('unified:a09fe73e-8aad-594a-a17f-682b961daad4','lcc','JQ',0),
('unified:a09fe73e-8aad-594a-a17f-682b961daad4','lcc','JQ21..JQ199.99@2',1);

CREATE TEMP TABLE jq_split(
  concept_id TEXT PRIMARY KEY,
  label TEXT NOT NULL,
  selector TEXT NOT NULL UNIQUE,
  display_order INTEGER NOT NULL
) WITHOUT ROWID;

INSERT INTO jq_split VALUES
('bokheim:politics:government:india','Indian Government & Administration','JQ200..JQ620.99@3',210),
('bokheim:politics:government:nepal','Nepalese Government & Administration','JQ628..JQ628.49@3',220),
('bokheim:politics:government:bhutan','Bhutanese Government & Administration','JQ628.5..JQ628.99@3',230),
('bokheim:politics:government:pakistan','Pakistani Government & Administration','JQ629..JQ629.99@3',240),
('bokheim:politics:government:bangladesh','Bangladeshi Government & Administration','JQ630..JQ639.49@3',250),
('bokheim:politics:government:maldives','Maldivian Government & Administration','JQ639.5..JQ639.99@3',260),
('bokheim:politics:government:sri-lanka','Sri Lankan Government & Administration','JQ650..JQ659.99@3',270),
('bokheim:politics:government:southeast-asia','Southeast Asian Government & Administration','JQ750..JQ1064.99@3',280),
('bokheim:politics:government:central-asia-siberia','Central Asian & Siberian Government','JQ1070..JQ1199.99@3',290),
('bokheim:politics:government:philippines','Philippine Government & Administration','JQ1250..JQ1419.99@3',300),
('bokheim:politics:government:east-asia-general','East Asian Government — General','JQ1499',310),
('bokheim:politics:government:china','Chinese Government & Administration','JQ1500..JQ1519.29@3',320),
('bokheim:politics:government:tibet-exile','Tibetan Government-in-Exile','JQ1519.3..JQ1519.49@3',330),
('bokheim:politics:government:macau','Macau Government & Administration','JQ1519.5..JQ1519.99@3',340),
('bokheim:politics:government:taiwan','Taiwanese Government & Administration','JQ1520..JQ1539.49@3',350),
('bokheim:politics:government:hong-kong','Hong Kong Government & Administration','JQ1539.5..JQ1599.99@3',360),
('bokheim:politics:government:japan','Japanese Government & Administration','JQ1600..JQ1699.99@3',370),
('bokheim:politics:government:korea','Korean Government & Administration','JQ1720..JQ1729.99@3',380),
('bokheim:politics:government:mongolia','Mongolian Government & Administration','JQ1730..JQ1739.99@3',390),
('bokheim:politics:government:thailand','Thai Government & Administration','JQ1740..JQ1749.99@3',400),
('bokheim:politics:government:middle-east-caucasus','Middle Eastern & Caucasian Government','JQ1758..JQ1852.99@3',410),
('bokheim:politics:government:africa','African Government & Administration','JQ1870..JQ3981.49@2',420),
('bokheim:politics:government:atlantic-islands','Atlantic Islands Government & Administration','JQ3981.5..JQ3986.7@3',430),
('bokheim:politics:government:australasia','Australasian Government & Administration','JQ3995..JQ5899.99@2',440),
('bokheim:politics:government:pacific-islands','Pacific Islands Government & Administration','JQ5995..JQ6651.99@2',450);

INSERT INTO concept(concept_id,preferred_label)
SELECT concept_id,label FROM jq_split;
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal,display_order)
SELECT concept_id,'bokheim:politics:government-administration',0,display_order
FROM jq_split;
INSERT INTO source_selector(concept_id,system_id,selector,ordinal)
SELECT concept_id,'lcc',selector,0 FROM jq_split;

COMMIT;

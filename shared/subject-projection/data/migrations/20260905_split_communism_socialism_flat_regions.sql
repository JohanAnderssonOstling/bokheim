PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;

-- The existing BISAC leaf becomes the general/theory/history leaf.
UPDATE concept SET preferred_label='Socialism & Communism — General, Theory & History'
WHERE concept_id='bokheim:politics:ideology:communism-socialism';
UPDATE source_selector SET selector='HX1..HX79@2'
WHERE concept_id='bokheim:politics:ideology:communism-socialism'
  AND system_id='lcc' AND selector='HX1..HX780.92@1';

CREATE TEMP TABLE hx_split(
 concept_id TEXT PRIMARY KEY,label TEXT NOT NULL,selector TEXT NOT NULL UNIQUE,display_order INTEGER NOT NULL
) WITHOUT ROWID;
INSERT INTO hx_split VALUES
('bokheim:politics:ideology:socialism-americas','Socialism & Communism — Americas','HX80..HX235@2',100),
('bokheim:politics:ideology:socialism-europe','Socialism & Communism — Europe','HX236..HX310.99@2',110),
('bokheim:politics:ideology:socialism-russia-soviet','Socialism & Communism — Russia & Former Soviet Sphere','HX311..HX315.99@2',120),
('bokheim:politics:ideology:socialism-europe-rest','Socialism & Communism — Europe','HX316..HX375.99@2',130),
('bokheim:politics:ideology:socialism-middle-east-central-asia','Socialism & Communism — Middle East & Central Asia','HX376..HX385.29@2',140),
('bokheim:politics:ideology:socialism-asia-pacific','Socialism & Communism — Asia & Pacific','HX385.3..HX435.99@2',150),
('bokheim:politics:ideology:socialism-africa','Socialism & Communism — Africa','HX436..HX470.99@2',160),
('bokheim:politics:ideology:socialism-oceania','Socialism & Communism — Australia & Oceania','HX471..HX517.99@2',170),
('bokheim:politics:ideology:socialism-society-culture','Socialism & Communism in Society & Culture','HX518..HX550.99@2',180),
('bokheim:politics:ideology:utopian-socialism-communities','Utopian Socialism & Collective Communities','HX626..HX780.92@2',190);

INSERT INTO concept SELECT concept_id,label FROM hx_split;
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal,display_order)
SELECT concept_id,'bokheim:politics:theory-ideologies',0,display_order FROM hx_split;
INSERT INTO source_selector(concept_id,system_id,selector,ordinal)
SELECT concept_id,'lcc',selector,0 FROM hx_split;

-- Europe is intentionally represented by two disjoint ranges on one leaf.
UPDATE source_selector
SET concept_id='bokheim:politics:ideology:socialism-europe',ordinal=1
WHERE concept_id='bokheim:politics:ideology:socialism-europe-rest';
DELETE FROM concept WHERE concept_id='bokheim:politics:ideology:socialism-europe-rest';

DROP TABLE hx_split;
COMMIT;

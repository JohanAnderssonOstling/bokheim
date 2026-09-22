-- Curate Greek history, retaining extra depth for Ancient and Byzantine periods.
PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;
CREATE TEMP TABLE greek_tree(concept_id TEXT PRIMARY KEY) WITHOUT ROWID;
WITH RECURSIVE t(id) AS (VALUES('unified:1773efe1-e08c-54df-a272-ac76cf96d508') UNION SELECT p.concept_id FROM t JOIN concept_parent p ON p.parent_concept_id=t.id)
INSERT INTO greek_tree SELECT id FROM t;

INSERT OR IGNORE INTO concept VALUES
('unified:f29549ad-d970-5b9d-bc8a-1d04b7f96b3f','Ancient Greece'),
('unified:f94f9394-6a40-5667-b600-a33cb82644c2','Byzantine Greece, 323–1453'),
('bokheim:history-lcc:e229749f-ed6e-5927-a31e-2ea7ae77600c','Ottoman Greece, 1453–1821'),
('unified:ae3f2284-3609-5646-a4f5-08bd5e26f56f','Modern Greece'),
('bokheim:history-lcc-full:f2fdab67-9d87-5b4d-92a9-809322a2e7f2','Bronze Age'),
('bokheim:history-lcc-full:8fb2c3b1-f8c5-5b2a-a4fd-7464a07214bf','Minoan Civilization'),
('bokheim:history-lcc-full:1bebec15-36f1-516a-96fb-d94580577cd2','Mycenaean Civilization'),
('bokheim:history-lcc-full:25fc1443-8a05-5142-829b-9d575c123c98','Archaic Greece, c. 800–500 BC'),
('bokheim:greece:classical','Classical Greece, 500–323 BC'),
('bokheim:history-lcc:df931e76-235f-5a60-9599-af6d8abc9392','Persian Wars, 499–479 BC'),
('bokheim:history-lcc:88d0b8e6-083a-591e-8624-1cd3954f68b6','Age of Pericles, 479–431 BC'),
('bokheim:history-lcc:1964fa40-1a07-5c53-9065-55a506a7404b','Peloponnesian War, 431–404 BC'),
('bokheim:history-lcc:5217047b-ec7d-54c4-9942-b18a4aa22a55','Spartan & Theban Supremacy, 404–362 BC'),
('bokheim:history-lcc:5e5be18d-c9f6-5d4c-8587-d11bc245d778','Macedonian Ascendancy, 359–336 BC'),
('bokheim:history-lcc:2fb7f234-a034-54c3-83c4-54773cde23b9','Alexander the Great, 336–323 BC'),
('bokheim:history-lcc:0f752b11-986b-58a5-9c3a-7d60a90179ee','Hellenistic Greece, 323–146 BC'),
('bokheim:history-lcc:a710b56e-f568-593d-a141-c4136ac1f25e','Roman Greece, 146 BC–323 AD'),
('bokheim:greece:early-byzantine','Early Byzantine, 323–720'),
('bokheim:history-lcc-full:cb731087-ebc4-5a83-a27b-70d1cbb7bcc2','Iconoclast Era, 720–886'),
('bokheim:history-lcc-full:ed279f36-ce88-5fe7-96c1-ddb8339831f0','Macedonian Era, 886–1057'),
('bokheim:history-lcc:141a8ae9-e6cf-5bc8-8dae-cc2fefa33e7d','Late Byzantine, 1057–1453'),
('bokheim:history-lcc:0d2116d8-77b4-5493-8b3b-9d62c12c237d','Latin Empire, 1204–1261'),
('bokheim:history-lcc:76379965-8fbb-59f7-ab51-ef4c4276f97e','Palaiologan Era, 1261–1453'),
('bokheim:history-lcc:7e8aea21-ee87-5724-8986-222a8cdccaf8','War of Independence, 1821–1829'),
('bokheim:greece:kingdom','Kingdom of Greece, 1832–1924'),
('bokheim:history-lcc:7564be93-8cf2-5206-857c-628a532a9b02','Second Republic, 1924–1935'),
('bokheim:greece:restoration-wwii','Restoration & World War II, 1935–1946'),
('bokheim:greece:civil-war','Civil War, 1946–1949'),
('bokheim:greece:postwar-kingdom','Postwar Kingdom, 1949–1967'),
('bokheim:history-lcc-full:7021f604-9afb-5640-80ed-7390780d54cd','Military Rule, 1967–1974'),
('bokheim:greece:third-republic','Third Republic, 1974–Present');

CREATE TEMP TABLE greek_keep(concept_id TEXT PRIMARY KEY) WITHOUT ROWID;
INSERT INTO greek_keep VALUES
('unified:1773efe1-e08c-54df-a272-ac76cf96d508'),
('unified:f29549ad-d970-5b9d-bc8a-1d04b7f96b3f'),('unified:f94f9394-6a40-5667-b600-a33cb82644c2'),('bokheim:history-lcc:e229749f-ed6e-5927-a31e-2ea7ae77600c'),('unified:ae3f2284-3609-5646-a4f5-08bd5e26f56f'),
('bokheim:history-lcc-full:f2fdab67-9d87-5b4d-92a9-809322a2e7f2'),('bokheim:history-lcc-full:8fb2c3b1-f8c5-5b2a-a4fd-7464a07214bf'),('bokheim:history-lcc-full:1bebec15-36f1-516a-96fb-d94580577cd2'),('bokheim:history-lcc-full:25fc1443-8a05-5142-829b-9d575c123c98'),('bokheim:greece:classical'),
('bokheim:history-lcc:df931e76-235f-5a60-9599-af6d8abc9392'),('bokheim:history-lcc:88d0b8e6-083a-591e-8624-1cd3954f68b6'),('bokheim:history-lcc:1964fa40-1a07-5c53-9065-55a506a7404b'),('bokheim:history-lcc:5217047b-ec7d-54c4-9942-b18a4aa22a55'),('bokheim:history-lcc:5e5be18d-c9f6-5d4c-8587-d11bc245d778'),('bokheim:history-lcc:2fb7f234-a034-54c3-83c4-54773cde23b9'),('bokheim:history-lcc:0f752b11-986b-58a5-9c3a-7d60a90179ee'),('bokheim:history-lcc:a710b56e-f568-593d-a141-c4136ac1f25e'),
('bokheim:greece:early-byzantine'),('bokheim:history-lcc-full:cb731087-ebc4-5a83-a27b-70d1cbb7bcc2'),('bokheim:history-lcc-full:ed279f36-ce88-5fe7-96c1-ddb8339831f0'),('bokheim:history-lcc:141a8ae9-e6cf-5bc8-8dae-cc2fefa33e7d'),('bokheim:history-lcc:0d2116d8-77b4-5493-8b3b-9d62c12c237d'),('bokheim:history-lcc:76379965-8fbb-59f7-ab51-ef4c4276f97e'),
('bokheim:history-lcc:7e8aea21-ee87-5724-8986-222a8cdccaf8'),('bokheim:greece:kingdom'),('bokheim:history-lcc:7564be93-8cf2-5206-857c-628a532a9b02'),('bokheim:greece:restoration-wwii'),('bokheim:greece:civil-war'),('bokheim:greece:postwar-kingdom'),('bokheim:history-lcc-full:7021f604-9afb-5640-80ed-7390780d54cd'),('bokheim:greece:third-republic');

CREATE TEMP TABLE greek_shared(concept_id TEXT PRIMARY KEY) WITHOUT ROWID;
WITH RECURSIVE s(id) AS (
 SELECT t.concept_id FROM greek_tree t JOIN concept_parent p ON p.concept_id=t.concept_id WHERE p.parent_concept_id NOT IN (SELECT concept_id FROM greek_tree) AND t.concept_id<>'unified:1773efe1-e08c-54df-a272-ac76cf96d508'
 UNION SELECT p.concept_id FROM s JOIN concept_parent p ON p.parent_concept_id=s.id WHERE p.concept_id IN (SELECT concept_id FROM greek_tree)
) INSERT INTO greek_shared SELECT id FROM s;
DELETE FROM concept_parent WHERE parent_concept_id IN (SELECT concept_id FROM greek_tree);
DELETE FROM concept_parent WHERE concept_id IN (SELECT concept_id FROM greek_tree WHERE concept_id NOT IN (SELECT concept_id FROM greek_keep) AND concept_id NOT IN (SELECT concept_id FROM greek_shared));
DELETE FROM concept WHERE concept_id IN (SELECT concept_id FROM greek_tree WHERE concept_id NOT IN (SELECT concept_id FROM greek_keep) AND concept_id NOT IN (SELECT concept_id FROM greek_shared));

CREATE TEMP TABLE greek_edge(child_id TEXT,parent_id TEXT,display_order INTEGER,PRIMARY KEY(child_id,parent_id)) WITHOUT ROWID;
INSERT INTO greek_edge VALUES
('unified:f29549ad-d970-5b9d-bc8a-1d04b7f96b3f','unified:1773efe1-e08c-54df-a272-ac76cf96d508',10),('unified:f94f9394-6a40-5667-b600-a33cb82644c2','unified:1773efe1-e08c-54df-a272-ac76cf96d508',20),('bokheim:history-lcc:e229749f-ed6e-5927-a31e-2ea7ae77600c','unified:1773efe1-e08c-54df-a272-ac76cf96d508',30),('unified:ae3f2284-3609-5646-a4f5-08bd5e26f56f','unified:1773efe1-e08c-54df-a272-ac76cf96d508',40),
('bokheim:history-lcc-full:f2fdab67-9d87-5b4d-92a9-809322a2e7f2','unified:f29549ad-d970-5b9d-bc8a-1d04b7f96b3f',10),('bokheim:history-lcc-full:25fc1443-8a05-5142-829b-9d575c123c98','unified:f29549ad-d970-5b9d-bc8a-1d04b7f96b3f',20),('bokheim:greece:classical','unified:f29549ad-d970-5b9d-bc8a-1d04b7f96b3f',30),('bokheim:history-lcc:0f752b11-986b-58a5-9c3a-7d60a90179ee','unified:f29549ad-d970-5b9d-bc8a-1d04b7f96b3f',40),('bokheim:history-lcc:a710b56e-f568-593d-a141-c4136ac1f25e','unified:f29549ad-d970-5b9d-bc8a-1d04b7f96b3f',50),
('bokheim:history-lcc-full:8fb2c3b1-f8c5-5b2a-a4fd-7464a07214bf','bokheim:history-lcc-full:f2fdab67-9d87-5b4d-92a9-809322a2e7f2',10),('bokheim:history-lcc-full:1bebec15-36f1-516a-96fb-d94580577cd2','bokheim:history-lcc-full:f2fdab67-9d87-5b4d-92a9-809322a2e7f2',20),
('bokheim:history-lcc:df931e76-235f-5a60-9599-af6d8abc9392','bokheim:greece:classical',10),('bokheim:history-lcc:88d0b8e6-083a-591e-8624-1cd3954f68b6','bokheim:greece:classical',20),('bokheim:history-lcc:1964fa40-1a07-5c53-9065-55a506a7404b','bokheim:greece:classical',30),('bokheim:history-lcc:5217047b-ec7d-54c4-9942-b18a4aa22a55','bokheim:greece:classical',40),('bokheim:history-lcc:5e5be18d-c9f6-5d4c-8587-d11bc245d778','bokheim:greece:classical',50),('bokheim:history-lcc:2fb7f234-a034-54c3-83c4-54773cde23b9','bokheim:greece:classical',60),
('bokheim:greece:early-byzantine','unified:f94f9394-6a40-5667-b600-a33cb82644c2',10),('bokheim:history-lcc-full:cb731087-ebc4-5a83-a27b-70d1cbb7bcc2','unified:f94f9394-6a40-5667-b600-a33cb82644c2',20),('bokheim:history-lcc-full:ed279f36-ce88-5fe7-96c1-ddb8339831f0','unified:f94f9394-6a40-5667-b600-a33cb82644c2',30),('bokheim:history-lcc:141a8ae9-e6cf-5bc8-8dae-cc2fefa33e7d','unified:f94f9394-6a40-5667-b600-a33cb82644c2',40),('bokheim:history-lcc:0d2116d8-77b4-5493-8b3b-9d62c12c237d','bokheim:history-lcc:141a8ae9-e6cf-5bc8-8dae-cc2fefa33e7d',10),('bokheim:history-lcc:76379965-8fbb-59f7-ab51-ef4c4276f97e','bokheim:history-lcc:141a8ae9-e6cf-5bc8-8dae-cc2fefa33e7d',20),
('bokheim:history-lcc:7e8aea21-ee87-5724-8986-222a8cdccaf8','unified:ae3f2284-3609-5646-a4f5-08bd5e26f56f',10),('bokheim:greece:kingdom','unified:ae3f2284-3609-5646-a4f5-08bd5e26f56f',20),('bokheim:history-lcc:7564be93-8cf2-5206-857c-628a532a9b02','unified:ae3f2284-3609-5646-a4f5-08bd5e26f56f',30),('bokheim:greece:restoration-wwii','unified:ae3f2284-3609-5646-a4f5-08bd5e26f56f',40),('bokheim:greece:civil-war','unified:ae3f2284-3609-5646-a4f5-08bd5e26f56f',50),('bokheim:greece:postwar-kingdom','unified:ae3f2284-3609-5646-a4f5-08bd5e26f56f',60),('bokheim:history-lcc-full:7021f604-9afb-5640-80ed-7390780d54cd','unified:ae3f2284-3609-5646-a4f5-08bd5e26f56f',70),('bokheim:greece:third-republic','unified:ae3f2284-3609-5646-a4f5-08bd5e26f56f',80);
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal,display_order) SELECT child_id,parent_id,COALESCE((SELECT max(ordinal)+1 FROM concept_parent WHERE concept_id=child_id),0),display_order FROM greek_edge;

UPDATE concept SET preferred_label=CASE concept_id
WHEN 'unified:f94f9394-6a40-5667-b600-a33cb82644c2' THEN 'Byzantine Greece, 323–1453' WHEN 'bokheim:history-lcc:e229749f-ed6e-5927-a31e-2ea7ae77600c' THEN 'Ottoman Greece, 1453–1821'
WHEN 'bokheim:history-lcc-full:f2fdab67-9d87-5b4d-92a9-809322a2e7f2' THEN 'Bronze Age' WHEN 'bokheim:history-lcc-full:8fb2c3b1-f8c5-5b2a-a4fd-7464a07214bf' THEN 'Minoan Civilization' WHEN 'bokheim:history-lcc-full:1bebec15-36f1-516a-96fb-d94580577cd2' THEN 'Mycenaean Civilization' WHEN 'bokheim:history-lcc-full:25fc1443-8a05-5142-829b-9d575c123c98' THEN 'Archaic Greece, c. 800–500 BC'
WHEN 'bokheim:history-lcc:df931e76-235f-5a60-9599-af6d8abc9392' THEN 'Persian Wars, 499–479 BC' WHEN 'bokheim:history-lcc:88d0b8e6-083a-591e-8624-1cd3954f68b6' THEN 'Age of Pericles, 479–431 BC' WHEN 'bokheim:history-lcc:1964fa40-1a07-5c53-9065-55a506a7404b' THEN 'Peloponnesian War, 431–404 BC' WHEN 'bokheim:history-lcc:5217047b-ec7d-54c4-9942-b18a4aa22a55' THEN 'Spartan & Theban Supremacy, 404–362 BC' WHEN 'bokheim:history-lcc:5e5be18d-c9f6-5d4c-8587-d11bc245d778' THEN 'Macedonian Ascendancy, 359–336 BC' WHEN 'bokheim:history-lcc:2fb7f234-a034-54c3-83c4-54773cde23b9' THEN 'Alexander the Great, 336–323 BC' WHEN 'bokheim:history-lcc:0f752b11-986b-58a5-9c3a-7d60a90179ee' THEN 'Hellenistic Greece, 323–146 BC' WHEN 'bokheim:history-lcc:a710b56e-f568-593d-a141-c4136ac1f25e' THEN 'Roman Greece, 146 BC–323 AD'
WHEN 'bokheim:history-lcc-full:cb731087-ebc4-5a83-a27b-70d1cbb7bcc2' THEN 'Iconoclast Era, 720–886' WHEN 'bokheim:history-lcc-full:ed279f36-ce88-5fe7-96c1-ddb8339831f0' THEN 'Macedonian Era, 886–1057' WHEN 'bokheim:history-lcc:141a8ae9-e6cf-5bc8-8dae-cc2fefa33e7d' THEN 'Late Byzantine, 1057–1453' WHEN 'bokheim:history-lcc:0d2116d8-77b4-5493-8b3b-9d62c12c237d' THEN 'Latin Empire, 1204–1261' WHEN 'bokheim:history-lcc:76379965-8fbb-59f7-ab51-ef4c4276f97e' THEN 'Palaiologan Era, 1261–1453'
WHEN 'bokheim:history-lcc:7e8aea21-ee87-5724-8986-222a8cdccaf8' THEN 'War of Independence, 1821–1829' WHEN 'bokheim:history-lcc:7564be93-8cf2-5206-857c-628a532a9b02' THEN 'Second Republic, 1924–1935' WHEN 'bokheim:history-lcc-full:7021f604-9afb-5640-80ed-7390780d54cd' THEN 'Military Rule, 1967–1974' ELSE preferred_label END
WHERE concept_id IN (SELECT concept_id FROM greek_keep);
DROP TABLE greek_edge; DROP TABLE greek_shared; DROP TABLE greek_keep; DROP TABLE greek_tree;
COMMIT;

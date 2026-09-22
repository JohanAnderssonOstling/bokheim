-- Curate the ten remaining European national/history trees into concise chronologies.
PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;

CREATE TEMP TABLE euro_root(root_id TEXT PRIMARY KEY,slug TEXT UNIQUE) WITHOUT ROWID;
INSERT INTO euro_root VALUES
('unified:0fbb9742-b8c8-52d7-8d1d-aa88c39ee3fc','norway'),
('unified:bfa4fe6f-f7d3-5e20-982b-fd1a308642b5','bulgaria'),
('unified:4ce7974f-9207-5786-8ea5-68f41923874c','romania'),
('unified:f9d54924-adb7-5cc4-8896-f87ed984e468','netherlands'),
('unified:4a84d354-9db9-5152-9897-c3af9dbcfa57','denmark'),
('unified:e280b450-322d-528d-afe3-f905673ed43b','malta'),
('unified:550fd12f-93fc-55dc-8a89-eefb55a0ff09','finland'),
('unified:d6a26c8f-e4c6-563b-9f68-9930c15a0641','iceland'),
('unified:a9b40fd5-19b3-5adc-8354-44d08510a2f0','luxembourg'),
('bokheim:history-lcc:8fad3838-2273-5eff-af3d-c0b64bd35c0c','yugoslavia');

CREATE TEMP TABLE euro_tree(root_id TEXT,concept_id TEXT,PRIMARY KEY(root_id,concept_id)) WITHOUT ROWID;
WITH RECURSIVE t(root_id,id) AS (SELECT root_id,root_id FROM euro_root UNION SELECT t.root_id,p.concept_id FROM t JOIN concept_parent p ON p.parent_concept_id=t.id)
INSERT INTO euro_tree SELECT root_id,id FROM t;

-- Preserve every removed external classification as a country-level fallback.
CREATE TEMP TABLE moved_selector(root_id TEXT,system_id TEXT,selector TEXT,PRIMARY KEY(root_id,system_id,selector)) WITHOUT ROWID;
INSERT OR IGNORE INTO moved_selector SELECT t.root_id,s.system_id,s.selector FROM euro_tree t JOIN source_selector s ON s.concept_id=t.concept_id WHERE t.concept_id<>t.root_id;
WITH numbered AS (
 SELECT m.*,row_number() OVER (PARTITION BY m.root_id,m.system_id ORDER BY m.selector)-1 AS rn
 FROM moved_selector m WHERE NOT EXISTS (SELECT 1 FROM source_selector s WHERE s.concept_id=m.root_id AND s.system_id=m.system_id AND s.selector=m.selector)
)
INSERT OR IGNORE INTO source_selector(concept_id,system_id,selector,ordinal)
SELECT n.root_id,n.system_id,n.selector,COALESCE((SELECT max(s.ordinal)+1 FROM source_selector s WHERE s.concept_id=n.root_id AND s.system_id=n.system_id),0)+n.rn FROM numbered n;

CREATE TEMP TABLE euro_shared(concept_id TEXT PRIMARY KEY) WITHOUT ROWID;
WITH RECURSIVE s(id) AS (
 SELECT DISTINCT t.concept_id FROM euro_tree t JOIN concept_parent p ON p.concept_id=t.concept_id
 WHERE t.concept_id<>t.root_id AND p.parent_concept_id NOT IN (SELECT concept_id FROM euro_tree)
 UNION SELECT p.concept_id FROM s JOIN concept_parent p ON p.parent_concept_id=s.id WHERE p.concept_id IN (SELECT concept_id FROM euro_tree)
) INSERT INTO euro_shared SELECT id FROM s;
DELETE FROM concept_parent WHERE parent_concept_id IN (SELECT root_id FROM euro_root);
DELETE FROM concept_parent WHERE parent_concept_id IN (SELECT concept_id FROM euro_tree) AND parent_concept_id NOT IN (SELECT concept_id FROM euro_shared);
DELETE FROM concept_parent WHERE concept_id IN (SELECT concept_id FROM euro_tree WHERE concept_id NOT IN (SELECT root_id FROM euro_root) AND concept_id NOT IN (SELECT concept_id FROM euro_shared));
DELETE FROM concept WHERE concept_id IN (SELECT concept_id FROM euro_tree WHERE concept_id NOT IN (SELECT root_id FROM euro_root) AND concept_id NOT IN (SELECT concept_id FROM euro_shared));

CREATE TEMP TABLE euro_period(slug TEXT,seq INTEGER,label TEXT,PRIMARY KEY(slug,seq)) WITHOUT ROWID;
INSERT INTO euro_period VALUES
('norway',10,'Early Norway, to 872'),('norway',20,'Viking Kingdom, 872–1035'),('norway',30,'Medieval Norway, 1035–1387'),('norway',40,'Danish Union, 1387–1814'),('norway',50,'Union with Sweden, 1814–1905'),('norway',60,'Independent Norway, 1905–1940'),('norway',70,'German Occupation, 1940–1945'),('norway',80,'Postwar Norway, 1945–1990'),('norway',90,'Contemporary Norway, 1990–Present'),
('bulgaria',10,'Early Bulgaria, to 681'),('bulgaria',20,'First Bulgarian Empire, 681–1018'),('bulgaria',30,'Byzantine Rule, 1018–1185'),('bulgaria',40,'Second Bulgarian Empire, 1185–1396'),('bulgaria',50,'Ottoman Rule, 1396–1878'),('bulgaria',60,'Principality & Kingdom, 1878–1944'),('bulgaria',70,'Communist Bulgaria, 1944–1990'),('bulgaria',80,'Republic of Bulgaria, 1990–Present'),
('romania',10,'Dacian & Roman Era, to 600'),('romania',20,'Medieval Principalities, 600–1601'),('romania',30,'Ottoman & Phanariote Era, 1601–1822'),('romania',40,'National Revival, 1822–1859'),('romania',50,'United Principalities, 1859–1881'),('romania',60,'Kingdom of Romania, 1881–1947'),('romania',70,'World War II, 1940–1944'),('romania',80,'Communist Romania, 1947–1989'),('romania',90,'Post-Communist Romania, 1989–Present'),
('netherlands',10,'Medieval Netherlands, to 1555'),('netherlands',20,'Dutch Revolt, 1555–1581'),('netherlands',30,'Dutch Republic, 1581–1795'),('netherlands',40,'Batavian Republic, 1795–1806'),('netherlands',50,'Kingdom of Holland, 1806–1810'),('netherlands',60,'French Rule, 1810–1813'),('netherlands',70,'United Netherlands, 1815–1830'),('netherlands',80,'Kingdom of the Netherlands, 1830–1940'),('netherlands',90,'German Occupation, 1940–1945'),('netherlands',100,'Postwar Netherlands, 1945–Present'),
('denmark',10,'Early Denmark, to 750'),('denmark',20,'Viking Age, 750–1042'),('denmark',30,'Medieval Denmark, 1042–1397'),('denmark',40,'Kalmar Union, 1397–1523'),('denmark',50,'Early Modern Denmark, 1523–1660'),('denmark',60,'Absolute Monarchy, 1660–1849'),('denmark',70,'Constitutional Denmark, 1849–1914'),('denmark',80,'World Wars Era, 1914–1945'),('denmark',90,'Postwar Denmark, 1945–1990'),('denmark',100,'Contemporary Denmark, 1990–Present'),
('malta',10,'Early Malta, to 870'),('malta',20,'Medieval Malta, 870–1530'),('malta',30,'Knights of Malta, 1530–1798'),('malta',40,'French Occupation, 1798–1800'),('malta',50,'British Malta, 1800–1964'),('malta',60,'Independent Malta, 1964–Present'),
('finland',10,'Early Finland, to 1249'),('finland',20,'Swedish Finland, 1249–1809'),('finland',30,'Grand Duchy of Finland, 1809–1917'),('finland',40,'Early Independence, 1917–1939'),('finland',50,'Winter War, 1939–1940'),('finland',60,'Continuation War, 1941–1944'),('finland',70,'Postwar Finland, 1945–1991'),('finland',80,'Contemporary Finland, 1991–Present'),
('iceland',10,'Settlement Era, 874–930'),('iceland',20,'Icelandic Commonwealth, 930–1262'),('iceland',30,'Norwegian & Danish Rule, 1262–1800'),('iceland',40,'National Awakening, 1801–1918'),('iceland',50,'Kingdom of Iceland, 1918–1944'),('iceland',60,'Republic of Iceland, 1944–Present'),
('luxembourg',10,'County of Luxembourg, to 1354'),('luxembourg',20,'Duchy of Luxembourg, 1354–1815'),('luxembourg',30,'Grand Duchy, 1815–1940'),('luxembourg',40,'German Occupation, 1940–1945'),('luxembourg',50,'Postwar Luxembourg, 1945–Present'),
('yugoslavia',10,'Kingdom of Yugoslavia, 1918–1941'),('yugoslavia',20,'Occupation & Resistance, 1941–1945'),('yugoslavia',30,'Socialist Yugoslavia, 1945–1980'),('yugoslavia',40,'Late Yugoslavia, 1980–1991'),('yugoslavia',50,'Yugoslav Wars, 1991–2001'),('yugoslavia',60,'Successor States, 1992–Present');

INSERT INTO concept(concept_id,preferred_label) SELECT 'bokheim:curated:'||slug||':'||seq,label FROM euro_period;
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal,display_order)
SELECT 'bokheim:curated:'||p.slug||':'||p.seq,r.root_id,0,p.seq FROM euro_period p JOIN euro_root r USING(slug);
UPDATE concept SET preferred_label='Netherlands' WHERE concept_id='unified:f9d54924-adb7-5cc4-8896-f87ed984e468';
UPDATE concept SET preferred_label='Malta' WHERE concept_id='unified:e280b450-322d-528d-afe3-f905673ed43b';

DROP TABLE euro_period; DROP TABLE euro_shared; DROP TABLE moved_selector; DROP TABLE euro_tree; DROP TABLE euro_root;
COMMIT;

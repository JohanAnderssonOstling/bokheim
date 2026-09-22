-- Flatten Slovak history to meaningful national periods.
PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;
CREATE TEMP TABLE sk_tree(concept_id TEXT PRIMARY KEY) WITHOUT ROWID;
WITH RECURSIVE t(id) AS (VALUES('unified:8ccb7f5e-08dc-5cf0-9c0e-ff88611d8d52') UNION SELECT p.concept_id FROM t JOIN concept_parent p ON p.parent_concept_id=t.id) INSERT INTO sk_tree SELECT id FROM t;

INSERT OR IGNORE INTO concept VALUES
('bokheim:slovakia:early','Early Slovakia, to 1526'),
('bokheim:slovakia:habsburg','Habsburg Hungary, 1526–1800'),
('bokheim:history-lcc-full:2cd45682-d428-508b-9159-909a40235653','Slovak National Awakening, 1800–1918'),
('bokheim:history-lcc-full:501f3664-b586-5e71-b837-cd8deac5b6cc','Czechoslovak Period, 1918–1939'),
('bokheim:history-lcc-full:b2c44927-bb0a-527a-b889-64406f97c064','Slovak State, 1939–1945'),
('bokheim:history-lcc-full:572d703b-cbf3-5b5a-a35e-725b8d9b1bbe','Postwar Czechoslovakia, 1945–1968'),
('bokheim:history-lcc-full:52d9af56-66af-5ae9-8ea1-3da64d0160bc','Federal Czechoslovakia, 1968–1989'),
('bokheim:history-lcc-full:8f0f8872-8c65-5083-ba14-dcb5c18a3abe','Democratic Transition, 1989–1992'),
('bokheim:slovakia:independent','Independent Slovakia, 1993–Present');

CREATE TEMP TABLE sk_keep(concept_id TEXT PRIMARY KEY) WITHOUT ROWID;
INSERT INTO sk_keep SELECT concept_id FROM concept WHERE concept_id='unified:8ccb7f5e-08dc-5cf0-9c0e-ff88611d8d52' OR concept_id LIKE 'bokheim:slovakia:%' OR concept_id IN
('bokheim:history-lcc-full:2cd45682-d428-508b-9159-909a40235653','bokheim:history-lcc-full:501f3664-b586-5e71-b837-cd8deac5b6cc','bokheim:history-lcc-full:b2c44927-bb0a-527a-b889-64406f97c064','bokheim:history-lcc-full:572d703b-cbf3-5b5a-a35e-725b8d9b1bbe','bokheim:history-lcc-full:52d9af56-66af-5ae9-8ea1-3da64d0160bc','bokheim:history-lcc-full:8f0f8872-8c65-5083-ba14-dcb5c18a3abe');

CREATE TEMP TABLE sk_shared(concept_id TEXT PRIMARY KEY) WITHOUT ROWID;
WITH RECURSIVE s(id) AS (SELECT t.concept_id FROM sk_tree t JOIN concept_parent p ON p.concept_id=t.concept_id WHERE p.parent_concept_id NOT IN (SELECT concept_id FROM sk_tree) AND t.concept_id<>'unified:8ccb7f5e-08dc-5cf0-9c0e-ff88611d8d52' UNION SELECT p.concept_id FROM s JOIN concept_parent p ON p.parent_concept_id=s.id WHERE p.concept_id IN (SELECT concept_id FROM sk_tree)) INSERT INTO sk_shared SELECT id FROM s;
DELETE FROM concept_parent WHERE parent_concept_id IN (SELECT concept_id FROM sk_tree);
DELETE FROM concept_parent WHERE concept_id IN (SELECT concept_id FROM sk_tree WHERE concept_id NOT IN (SELECT concept_id FROM sk_keep) AND concept_id NOT IN (SELECT concept_id FROM sk_shared));
DELETE FROM concept WHERE concept_id IN (SELECT concept_id FROM sk_tree WHERE concept_id NOT IN (SELECT concept_id FROM sk_keep) AND concept_id NOT IN (SELECT concept_id FROM sk_shared));

CREATE TEMP TABLE sk_edge(child TEXT,parent TEXT,ord INTEGER,PRIMARY KEY(child,parent)) WITHOUT ROWID;
INSERT INTO sk_edge VALUES
('bokheim:slovakia:early','unified:8ccb7f5e-08dc-5cf0-9c0e-ff88611d8d52',10),
('bokheim:slovakia:habsburg','unified:8ccb7f5e-08dc-5cf0-9c0e-ff88611d8d52',20),
('bokheim:history-lcc-full:2cd45682-d428-508b-9159-909a40235653','unified:8ccb7f5e-08dc-5cf0-9c0e-ff88611d8d52',30),
('bokheim:history-lcc-full:501f3664-b586-5e71-b837-cd8deac5b6cc','unified:8ccb7f5e-08dc-5cf0-9c0e-ff88611d8d52',40),
('bokheim:history-lcc-full:b2c44927-bb0a-527a-b889-64406f97c064','unified:8ccb7f5e-08dc-5cf0-9c0e-ff88611d8d52',50),
('bokheim:history-lcc-full:572d703b-cbf3-5b5a-a35e-725b8d9b1bbe','unified:8ccb7f5e-08dc-5cf0-9c0e-ff88611d8d52',60),
('bokheim:history-lcc-full:52d9af56-66af-5ae9-8ea1-3da64d0160bc','unified:8ccb7f5e-08dc-5cf0-9c0e-ff88611d8d52',70),
('bokheim:history-lcc-full:8f0f8872-8c65-5083-ba14-dcb5c18a3abe','unified:8ccb7f5e-08dc-5cf0-9c0e-ff88611d8d52',80),
('bokheim:slovakia:independent','unified:8ccb7f5e-08dc-5cf0-9c0e-ff88611d8d52',90);
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal,display_order) SELECT child,parent,COALESCE((SELECT max(ordinal)+1 FROM concept_parent WHERE concept_id=child),0),ord FROM sk_edge;

UPDATE concept SET preferred_label=CASE concept_id
WHEN 'bokheim:history-lcc-full:2cd45682-d428-508b-9159-909a40235653' THEN 'Slovak National Awakening, 1800–1918'
WHEN 'bokheim:history-lcc-full:501f3664-b586-5e71-b837-cd8deac5b6cc' THEN 'Czechoslovak Period, 1918–1939'
WHEN 'bokheim:history-lcc-full:b2c44927-bb0a-527a-b889-64406f97c064' THEN 'Slovak State, 1939–1945'
WHEN 'bokheim:history-lcc-full:572d703b-cbf3-5b5a-a35e-725b8d9b1bbe' THEN 'Postwar Czechoslovakia, 1945–1968'
WHEN 'bokheim:history-lcc-full:52d9af56-66af-5ae9-8ea1-3da64d0160bc' THEN 'Federal Czechoslovakia, 1968–1989'
WHEN 'bokheim:history-lcc-full:8f0f8872-8c65-5083-ba14-dcb5c18a3abe' THEN 'Democratic Transition, 1989–1992'
ELSE preferred_label END WHERE concept_id IN (SELECT concept_id FROM sk_keep);
DROP TABLE sk_edge; DROP TABLE sk_shared; DROP TABLE sk_keep; DROP TABLE sk_tree;
COMMIT;

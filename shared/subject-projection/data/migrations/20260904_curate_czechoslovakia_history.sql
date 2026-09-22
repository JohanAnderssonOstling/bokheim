-- Curate Czechoslovakia into meaningful chronological periods.
PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;
CREATE TEMP TABLE cz_tree(concept_id TEXT PRIMARY KEY) WITHOUT ROWID;
WITH RECURSIVE t(id) AS (VALUES('unified:3e65feaf-765f-54bb-88bb-bc3ae2b8452d') UNION SELECT p.concept_id FROM t JOIN concept_parent p ON p.parent_concept_id=t.id)
INSERT INTO cz_tree SELECT id FROM t;

INSERT OR IGNORE INTO concept VALUES
('bokheim:history-lcc:3689d40e-4bfc-56df-bb75-b4b8d5aee1b5','Early & Medieval, to 1526'),
('bokheim:history-lcc-full:36e221b7-43e4-59d6-beba-324857cad85c','Přemyslid Dynasty, c. 900–1310'),
('bokheim:history-lcc-full:79c55173-3489-51d3-9995-b932e1051e36','Luxembourg Dynasty, 1311–1437'),
('bokheim:history-lcc-full:ab2678f6-f2f6-5a17-b449-37e3b7d3628d','Late Medieval, 1437–1526'),
('bokheim:history-lcc:070dd476-8617-583c-b39e-2c27eda5f953','Habsburg Rule, 1526–1918'),
('bokheim:czechoslovakia:early-habsburg','Early Habsburg Rule, 1526–1648'),
('bokheim:czechoslovakia:austrian-rule','Austrian Rule, 1648–1867'),
('bokheim:czechoslovakia:austria-hungary','Austria-Hungary, 1867–1918'),
('bokheim:history-lcc:b8649c59-7dfe-5465-9b41-c871702f3a9e','Czechoslovakia, 1918–1992'),
('bokheim:history-lcc-full:3f1e25ff-dbce-57a9-bb93-7550ea025ae5','First Republic, 1918–1938'),
('bokheim:history-lcc-full:eeac378a-51d7-5abb-b3c9-05d7032cb745','Munich Crisis, 1938'),
('bokheim:history-lcc-full:b88f0d20-8ebe-52dd-b5d9-fe089db31629','German Occupation, 1939–1945'),
('bokheim:czechoslovakia:postwar','Postwar Republic, 1945–1948'),
('bokheim:czechoslovakia:communist-early','Communist Rule, 1948–1968'),
('bokheim:czechoslovakia:prague-spring','Prague Spring, 1968'),
('bokheim:history-lcc-full:ec700ac8-af8b-5c27-a3e8-ca9acea1eb89','Normalization, 1968–1989'),
('bokheim:history-lcc-full:2b365b25-d39c-550d-bcc4-f541ac4a4704','Velvet Revolution, 1989'),
('bokheim:history-lcc-full:64b4321e-abb6-5cce-9b0f-e87a2209719f','Democratic Federation, 1989–1992');

CREATE TEMP TABLE cz_keep(concept_id TEXT PRIMARY KEY) WITHOUT ROWID;
INSERT INTO cz_keep VALUES
('unified:3e65feaf-765f-54bb-88bb-bc3ae2b8452d'),
('bokheim:history-lcc:3689d40e-4bfc-56df-bb75-b4b8d5aee1b5'),('bokheim:history-lcc-full:36e221b7-43e4-59d6-beba-324857cad85c'),('bokheim:history-lcc-full:79c55173-3489-51d3-9995-b932e1051e36'),('bokheim:history-lcc-full:ab2678f6-f2f6-5a17-b449-37e3b7d3628d'),
('bokheim:history-lcc:070dd476-8617-583c-b39e-2c27eda5f953'),('bokheim:czechoslovakia:early-habsburg'),('bokheim:czechoslovakia:austrian-rule'),('bokheim:czechoslovakia:austria-hungary'),
('bokheim:history-lcc:b8649c59-7dfe-5465-9b41-c871702f3a9e'),('bokheim:history-lcc-full:3f1e25ff-dbce-57a9-bb93-7550ea025ae5'),('bokheim:history-lcc-full:eeac378a-51d7-5abb-b3c9-05d7032cb745'),('bokheim:history-lcc-full:b88f0d20-8ebe-52dd-b5d9-fe089db31629'),('bokheim:czechoslovakia:postwar'),('bokheim:czechoslovakia:communist-early'),('bokheim:czechoslovakia:prague-spring'),('bokheim:history-lcc-full:ec700ac8-af8b-5c27-a3e8-ca9acea1eb89'),('bokheim:history-lcc-full:2b365b25-d39c-550d-bcc4-f541ac4a4704'),('bokheim:history-lcc-full:64b4321e-abb6-5cce-9b0f-e87a2209719f');

CREATE TEMP TABLE cz_shared(concept_id TEXT PRIMARY KEY) WITHOUT ROWID;
WITH RECURSIVE s(id) AS (
 SELECT t.concept_id FROM cz_tree t JOIN concept_parent p ON p.concept_id=t.concept_id WHERE p.parent_concept_id NOT IN (SELECT concept_id FROM cz_tree) AND t.concept_id<>'unified:3e65feaf-765f-54bb-88bb-bc3ae2b8452d'
 UNION SELECT p.concept_id FROM s JOIN concept_parent p ON p.parent_concept_id=s.id WHERE p.concept_id IN (SELECT concept_id FROM cz_tree)
) INSERT INTO cz_shared SELECT id FROM s;
DELETE FROM concept_parent WHERE parent_concept_id IN (SELECT concept_id FROM cz_tree);
DELETE FROM concept_parent WHERE concept_id IN (SELECT concept_id FROM cz_tree WHERE concept_id NOT IN (SELECT concept_id FROM cz_keep) AND concept_id NOT IN (SELECT concept_id FROM cz_shared));
DELETE FROM concept WHERE concept_id IN (SELECT concept_id FROM cz_tree WHERE concept_id NOT IN (SELECT concept_id FROM cz_keep) AND concept_id NOT IN (SELECT concept_id FROM cz_shared));

CREATE TEMP TABLE cz_edge(child_id TEXT,parent_id TEXT,display_order INTEGER,PRIMARY KEY(child_id,parent_id)) WITHOUT ROWID;
INSERT INTO cz_edge VALUES
('bokheim:history-lcc:3689d40e-4bfc-56df-bb75-b4b8d5aee1b5','unified:3e65feaf-765f-54bb-88bb-bc3ae2b8452d',10),
('bokheim:history-lcc:070dd476-8617-583c-b39e-2c27eda5f953','unified:3e65feaf-765f-54bb-88bb-bc3ae2b8452d',20),
('bokheim:history-lcc:b8649c59-7dfe-5465-9b41-c871702f3a9e','unified:3e65feaf-765f-54bb-88bb-bc3ae2b8452d',30),
('bokheim:history-lcc-full:36e221b7-43e4-59d6-beba-324857cad85c','bokheim:history-lcc:3689d40e-4bfc-56df-bb75-b4b8d5aee1b5',10),
('bokheim:history-lcc-full:79c55173-3489-51d3-9995-b932e1051e36','bokheim:history-lcc:3689d40e-4bfc-56df-bb75-b4b8d5aee1b5',20),
('bokheim:history-lcc-full:ab2678f6-f2f6-5a17-b449-37e3b7d3628d','bokheim:history-lcc:3689d40e-4bfc-56df-bb75-b4b8d5aee1b5',30),
('bokheim:czechoslovakia:early-habsburg','bokheim:history-lcc:070dd476-8617-583c-b39e-2c27eda5f953',10),
('bokheim:czechoslovakia:austrian-rule','bokheim:history-lcc:070dd476-8617-583c-b39e-2c27eda5f953',20),
('bokheim:czechoslovakia:austria-hungary','bokheim:history-lcc:070dd476-8617-583c-b39e-2c27eda5f953',30),
('bokheim:history-lcc-full:3f1e25ff-dbce-57a9-bb93-7550ea025ae5','bokheim:history-lcc:b8649c59-7dfe-5465-9b41-c871702f3a9e',10),
('bokheim:history-lcc-full:eeac378a-51d7-5abb-b3c9-05d7032cb745','bokheim:history-lcc:b8649c59-7dfe-5465-9b41-c871702f3a9e',20),
('bokheim:history-lcc-full:b88f0d20-8ebe-52dd-b5d9-fe089db31629','bokheim:history-lcc:b8649c59-7dfe-5465-9b41-c871702f3a9e',30),
('bokheim:czechoslovakia:postwar','bokheim:history-lcc:b8649c59-7dfe-5465-9b41-c871702f3a9e',40),
('bokheim:czechoslovakia:communist-early','bokheim:history-lcc:b8649c59-7dfe-5465-9b41-c871702f3a9e',50),
('bokheim:czechoslovakia:prague-spring','bokheim:history-lcc:b8649c59-7dfe-5465-9b41-c871702f3a9e',60),
('bokheim:history-lcc-full:ec700ac8-af8b-5c27-a3e8-ca9acea1eb89','bokheim:history-lcc:b8649c59-7dfe-5465-9b41-c871702f3a9e',70),
('bokheim:history-lcc-full:2b365b25-d39c-550d-bcc4-f541ac4a4704','bokheim:history-lcc:b8649c59-7dfe-5465-9b41-c871702f3a9e',80),
('bokheim:history-lcc-full:64b4321e-abb6-5cce-9b0f-e87a2209719f','bokheim:history-lcc:b8649c59-7dfe-5465-9b41-c871702f3a9e',90);
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal,display_order) SELECT child_id,parent_id,COALESCE((SELECT max(ordinal)+1 FROM concept_parent WHERE concept_id=child_id),0),display_order FROM cz_edge;

UPDATE concept SET preferred_label=CASE concept_id
WHEN 'bokheim:history-lcc:3689d40e-4bfc-56df-bb75-b4b8d5aee1b5' THEN 'Early & Medieval, to 1526'
WHEN 'bokheim:history-lcc-full:36e221b7-43e4-59d6-beba-324857cad85c' THEN 'Přemyslid Dynasty, c. 900–1310'
WHEN 'bokheim:history-lcc-full:79c55173-3489-51d3-9995-b932e1051e36' THEN 'Luxembourg Dynasty, 1311–1437'
WHEN 'bokheim:history-lcc-full:ab2678f6-f2f6-5a17-b449-37e3b7d3628d' THEN 'Late Medieval, 1437–1526'
WHEN 'bokheim:history-lcc:070dd476-8617-583c-b39e-2c27eda5f953' THEN 'Habsburg Rule, 1526–1918'
WHEN 'bokheim:history-lcc:b8649c59-7dfe-5465-9b41-c871702f3a9e' THEN 'Czechoslovakia, 1918–1992'
WHEN 'bokheim:history-lcc-full:3f1e25ff-dbce-57a9-bb93-7550ea025ae5' THEN 'First Republic, 1918–1938'
WHEN 'bokheim:history-lcc-full:eeac378a-51d7-5abb-b3c9-05d7032cb745' THEN 'Munich Crisis, 1938'
WHEN 'bokheim:history-lcc-full:b88f0d20-8ebe-52dd-b5d9-fe089db31629' THEN 'German Occupation, 1939–1945'
WHEN 'bokheim:history-lcc-full:ec700ac8-af8b-5c27-a3e8-ca9acea1eb89' THEN 'Normalization, 1968–1989'
WHEN 'bokheim:history-lcc-full:64b4321e-abb6-5cce-9b0f-e87a2209719f' THEN 'Democratic Federation, 1989–1992'
ELSE preferred_label END WHERE concept_id IN (SELECT concept_id FROM cz_keep);
DROP TABLE cz_edge; DROP TABLE cz_shared; DROP TABLE cz_keep; DROP TABLE cz_tree;
COMMIT;

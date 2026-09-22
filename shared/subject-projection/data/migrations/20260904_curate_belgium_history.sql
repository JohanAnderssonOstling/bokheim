-- Curate Belgium into meaningful chronological periods.
PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;
CREATE TEMP TABLE be_tree(concept_id TEXT PRIMARY KEY) WITHOUT ROWID;
WITH RECURSIVE t(id) AS (VALUES('unified:ecf16473-70d0-54e4-831c-b49dc25bb9ea') UNION SELECT p.concept_id FROM t JOIN concept_parent p ON p.parent_concept_id=t.id) INSERT INTO be_tree SELECT id FROM t;

INSERT OR IGNORE INTO concept VALUES
('bokheim:history-lcc-full:4a940c66-f40e-5bf7-94d0-2d785fe0eec1','Roman Belgium'),
('bokheim:history-lcc-full:4afabd33-fe7f-5e88-bc2e-079ad9adf711','Frankish Period'),
('bokheim:belgium:medieval','Medieval Low Countries, 843–1555'),
('bokheim:history-lcc-full:20cef0ba-b271-5686-8e45-cd45e956d697','Early Medieval, 843–1100'),
('bokheim:history-lcc-full:3123b5f4-8eb8-57c4-8a58-5ad43468b0d7','High Medieval, 1101–1400'),
('bokheim:history-lcc-full:ad452e6e-0614-58d1-9a27-66cc414befa1','Burgundian Netherlands, 1401–1506'),
('bokheim:history-lcc-full:d28dfc72-46a0-5eb7-b592-23160ac9cb70','Early Habsburg Rule, 1506–1555'),
('bokheim:belgium:spanish-netherlands','Spanish Netherlands, 1555–1714'),
('bokheim:history-lcc:6a6e1c3f-24ce-5b7f-8650-a158513e3e95','Austrian Netherlands, 1714–1794'),
('bokheim:history-lcc-full:00f3c613-3442-5089-9896-cb23fff66722','Brabant Revolution, 1789–1790'),
('bokheim:history-lcc:4f6fb6e5-a87a-5c00-8233-9f5c3a8737cf','French Rule, 1794–1815'),
('bokheim:history-lcc-full:0776dede-adf4-575c-8cb5-6929811116ad','United Kingdom of the Netherlands, 1815–1830'),
('bokheim:history-lcc:c956fd0a-3cc8-5a1e-8830-4cd91b04e529','Independent Belgium, 1830–Present'),
('bokheim:history-lcc:a8926f19-08d5-52d8-be6a-e9325c685642','Belgian Revolution, 1830'),
('bokheim:belgium:industrial','Industrial Belgium, 1830–1914'),
('bokheim:belgium:wwi','World War I, 1914–1918'),
('bokheim:belgium:interwar','Interwar Belgium, 1918–1940'),
('bokheim:belgium:wwii','World War II, 1940–1945'),
('bokheim:belgium:postwar','Postwar Belgium, 1945–1970'),
('bokheim:belgium:federalization','Federalization, 1970–Present');

CREATE TEMP TABLE be_keep(concept_id TEXT PRIMARY KEY) WITHOUT ROWID;
INSERT INTO be_keep SELECT concept_id FROM concept WHERE concept_id='unified:ecf16473-70d0-54e4-831c-b49dc25bb9ea' OR concept_id LIKE 'bokheim:belgium:%' OR concept_id IN
('bokheim:history-lcc-full:4a940c66-f40e-5bf7-94d0-2d785fe0eec1','bokheim:history-lcc-full:4afabd33-fe7f-5e88-bc2e-079ad9adf711','bokheim:history-lcc-full:20cef0ba-b271-5686-8e45-cd45e956d697','bokheim:history-lcc-full:3123b5f4-8eb8-57c4-8a58-5ad43468b0d7','bokheim:history-lcc-full:ad452e6e-0614-58d1-9a27-66cc414befa1','bokheim:history-lcc-full:d28dfc72-46a0-5eb7-b592-23160ac9cb70','bokheim:history-lcc:6a6e1c3f-24ce-5b7f-8650-a158513e3e95','bokheim:history-lcc-full:00f3c613-3442-5089-9896-cb23fff66722','bokheim:history-lcc:4f6fb6e5-a87a-5c00-8233-9f5c3a8737cf','bokheim:history-lcc-full:0776dede-adf4-575c-8cb5-6929811116ad','bokheim:history-lcc:c956fd0a-3cc8-5a1e-8830-4cd91b04e529','bokheim:history-lcc:a8926f19-08d5-52d8-be6a-e9325c685642');

CREATE TEMP TABLE be_shared(concept_id TEXT PRIMARY KEY) WITHOUT ROWID;
WITH RECURSIVE s(id) AS (SELECT t.concept_id FROM be_tree t JOIN concept_parent p ON p.concept_id=t.concept_id WHERE p.parent_concept_id NOT IN (SELECT concept_id FROM be_tree) AND t.concept_id<>'unified:ecf16473-70d0-54e4-831c-b49dc25bb9ea' UNION SELECT p.concept_id FROM s JOIN concept_parent p ON p.parent_concept_id=s.id WHERE p.concept_id IN (SELECT concept_id FROM be_tree)) INSERT INTO be_shared SELECT id FROM s;
DELETE FROM concept_parent WHERE parent_concept_id IN (SELECT concept_id FROM be_tree);
DELETE FROM concept_parent WHERE concept_id IN (SELECT concept_id FROM be_tree WHERE concept_id NOT IN (SELECT concept_id FROM be_keep) AND concept_id NOT IN (SELECT concept_id FROM be_shared));
DELETE FROM concept WHERE concept_id IN (SELECT concept_id FROM be_tree WHERE concept_id NOT IN (SELECT concept_id FROM be_keep) AND concept_id NOT IN (SELECT concept_id FROM be_shared));

CREATE TEMP TABLE be_edge(child TEXT,parent TEXT,ord INTEGER,PRIMARY KEY(child,parent)) WITHOUT ROWID;
INSERT INTO be_edge VALUES
('bokheim:history-lcc-full:4a940c66-f40e-5bf7-94d0-2d785fe0eec1','unified:ecf16473-70d0-54e4-831c-b49dc25bb9ea',10),('bokheim:history-lcc-full:4afabd33-fe7f-5e88-bc2e-079ad9adf711','unified:ecf16473-70d0-54e4-831c-b49dc25bb9ea',20),('bokheim:belgium:medieval','unified:ecf16473-70d0-54e4-831c-b49dc25bb9ea',30),('bokheim:belgium:spanish-netherlands','unified:ecf16473-70d0-54e4-831c-b49dc25bb9ea',40),('bokheim:history-lcc:6a6e1c3f-24ce-5b7f-8650-a158513e3e95','unified:ecf16473-70d0-54e4-831c-b49dc25bb9ea',50),('bokheim:history-lcc:4f6fb6e5-a87a-5c00-8233-9f5c3a8737cf','unified:ecf16473-70d0-54e4-831c-b49dc25bb9ea',60),('bokheim:history-lcc-full:0776dede-adf4-575c-8cb5-6929811116ad','unified:ecf16473-70d0-54e4-831c-b49dc25bb9ea',70),('bokheim:history-lcc:c956fd0a-3cc8-5a1e-8830-4cd91b04e529','unified:ecf16473-70d0-54e4-831c-b49dc25bb9ea',80),
('bokheim:history-lcc-full:20cef0ba-b271-5686-8e45-cd45e956d697','bokheim:belgium:medieval',10),('bokheim:history-lcc-full:3123b5f4-8eb8-57c4-8a58-5ad43468b0d7','bokheim:belgium:medieval',20),('bokheim:history-lcc-full:ad452e6e-0614-58d1-9a27-66cc414befa1','bokheim:belgium:medieval',30),('bokheim:history-lcc-full:d28dfc72-46a0-5eb7-b592-23160ac9cb70','bokheim:belgium:medieval',40),
('bokheim:history-lcc-full:00f3c613-3442-5089-9896-cb23fff66722','bokheim:history-lcc:6a6e1c3f-24ce-5b7f-8650-a158513e3e95',10),
('bokheim:history-lcc:a8926f19-08d5-52d8-be6a-e9325c685642','bokheim:history-lcc:c956fd0a-3cc8-5a1e-8830-4cd91b04e529',10),('bokheim:belgium:industrial','bokheim:history-lcc:c956fd0a-3cc8-5a1e-8830-4cd91b04e529',20),('bokheim:belgium:wwi','bokheim:history-lcc:c956fd0a-3cc8-5a1e-8830-4cd91b04e529',30),('bokheim:belgium:interwar','bokheim:history-lcc:c956fd0a-3cc8-5a1e-8830-4cd91b04e529',40),('bokheim:belgium:wwii','bokheim:history-lcc:c956fd0a-3cc8-5a1e-8830-4cd91b04e529',50),('bokheim:belgium:postwar','bokheim:history-lcc:c956fd0a-3cc8-5a1e-8830-4cd91b04e529',60),('bokheim:belgium:federalization','bokheim:history-lcc:c956fd0a-3cc8-5a1e-8830-4cd91b04e529',70);
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal,display_order) SELECT child,parent,COALESCE((SELECT max(ordinal)+1 FROM concept_parent WHERE concept_id=child),0),ord FROM be_edge;

UPDATE concept SET preferred_label=CASE concept_id WHEN 'bokheim:history-lcc-full:4a940c66-f40e-5bf7-94d0-2d785fe0eec1' THEN 'Roman Belgium' WHEN 'bokheim:history-lcc-full:4afabd33-fe7f-5e88-bc2e-079ad9adf711' THEN 'Frankish Period' WHEN 'bokheim:history-lcc-full:20cef0ba-b271-5686-8e45-cd45e956d697' THEN 'Early Medieval, 843–1100' WHEN 'bokheim:history-lcc-full:3123b5f4-8eb8-57c4-8a58-5ad43468b0d7' THEN 'High Medieval, 1101–1400' WHEN 'bokheim:history-lcc-full:ad452e6e-0614-58d1-9a27-66cc414befa1' THEN 'Burgundian Netherlands, 1401–1506' WHEN 'bokheim:history-lcc-full:d28dfc72-46a0-5eb7-b592-23160ac9cb70' THEN 'Early Habsburg Rule, 1506–1555' WHEN 'bokheim:history-lcc:6a6e1c3f-24ce-5b7f-8650-a158513e3e95' THEN 'Austrian Netherlands, 1714–1794' WHEN 'bokheim:history-lcc-full:00f3c613-3442-5089-9896-cb23fff66722' THEN 'Brabant Revolution, 1789–1790' WHEN 'bokheim:history-lcc:4f6fb6e5-a87a-5c00-8233-9f5c3a8737cf' THEN 'French Rule, 1794–1815' WHEN 'bokheim:history-lcc-full:0776dede-adf4-575c-8cb5-6929811116ad' THEN 'United Kingdom of the Netherlands, 1815–1830' WHEN 'bokheim:history-lcc:c956fd0a-3cc8-5a1e-8830-4cd91b04e529' THEN 'Independent Belgium, 1830–Present' WHEN 'bokheim:history-lcc:a8926f19-08d5-52d8-be6a-e9325c685642' THEN 'Belgian Revolution, 1830' ELSE preferred_label END WHERE concept_id IN (SELECT concept_id FROM be_keep);
DROP TABLE be_edge; DROP TABLE be_shared; DROP TABLE be_keep; DROP TABLE be_tree;
COMMIT;

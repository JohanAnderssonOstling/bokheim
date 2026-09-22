-- Curate France into a compact, period-only browsing tree.
PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;

CREATE TEMP TABLE france_tree(concept_id TEXT PRIMARY KEY) WITHOUT ROWID;
WITH RECURSIVE tree(concept_id) AS (
 VALUES('unified:3754b57e-8d6a-5d89-b8bd-cedbb34ef143')
 UNION SELECT p.concept_id FROM tree t JOIN concept_parent p ON p.parent_concept_id=t.concept_id
)
INSERT INTO france_tree SELECT concept_id FROM tree;

INSERT OR IGNORE INTO concept(concept_id,preferred_label) VALUES
('unified:743ac652-6402-5c66-b1ed-a3aff95fcb5a','Early & Medieval'),
('bokheim:france:early-modern','Early Modern, 1515–1789'),
('bokheim:france:revolution-napoleon','Revolution & Napoleon, 1789–1815'),
('bokheim:france:modern','Modern, 1815–Present'),
('bokheim:history-lcc-full:eabaaab4-cd7d-5dce-9990-79fcd8fb90fe','Gaul & Celts'),
('bokheim:history-lcc-full:e751b1ad-86ff-54fb-9c28-bcdb92106ac5','Frankish Period'),
('unified:e2f5915d-cc6e-54eb-8cf9-f1ac1fd1ba34','Late Medieval, 1328–1515'),
('bokheim:history-lcc:e739602c-c1b6-59cc-b594-d2759a263bf6','Hundred Years’ War, 1337–1453'),
('unified:8e12f4e1-058c-5a8c-bcb6-a65df85b172f','16th Century'),
('bokheim:lcc-detail:e8b0733e-757c-5a7f-9ff0-f691db9ee982','1589–1715'),
('bokheim:lcc-detail:878d9585-639b-5a71-8f2c-66c48191e600','1715–1789'),
('bokheim:france:french-revolution','French Revolution'),
('bokheim:france:first-republic','First Republic, 1792–1804'),
('bokheim:history-lcc-full:4a0acaa2-4d4b-5c70-8b69-cb830e7b1808','First Empire, 1804–1815'),
('bokheim:lcc-detail:973fa435-545d-58cc-b49b-1f0d42553bce','Bourbon Restoration, 1815–1830'),
('bokheim:lcc-detail:7d3b2024-54c4-52b0-8962-d7d6a0296a0f','July Monarchy, 1830–1848'),
('bokheim:history-lcc:267a50e3-ac40-54fe-b757-b38e4e43c3d5','Second Republic, 1848–1851'),
('bokheim:lcc-detail:ea2940eb-0f9e-54ca-9e23-9824e95763f0','Second Empire, 1852–1870'),
('bokheim:lcc-detail:2ed7ae64-1c47-5f80-b0b3-3d944240f22c','Franco-Prussian War, 1870–1871'),
('bokheim:history-lcc:282f1b24-40a0-5992-bf37-ccc03faaa3b4','Third Republic, 1871–1940'),
('bokheim:history-lcc:c559e7ac-482b-5400-84a6-e0b4b9d5aefd','Occupation & Liberation, 1940–1946'),
('bokheim:history-lcc:d2636f2a-236a-5b0c-8a1c-bf19c9519ea0','Fourth Republic, 1946–1958'),
('bokheim:history-lcc:47de3534-44ee-51c5-9805-37d13f02405d','Fifth Republic, 1958–Present');

CREATE TEMP TABLE france_keep(concept_id TEXT PRIMARY KEY) WITHOUT ROWID;
INSERT INTO france_keep SELECT concept_id FROM concept WHERE concept_id IN (
'unified:3754b57e-8d6a-5d89-b8bd-cedbb34ef143','unified:743ac652-6402-5c66-b1ed-a3aff95fcb5a',
'bokheim:france:early-modern','bokheim:france:revolution-napoleon','bokheim:france:modern',
'bokheim:history-lcc-full:eabaaab4-cd7d-5dce-9990-79fcd8fb90fe','bokheim:history-lcc-full:e751b1ad-86ff-54fb-9c28-bcdb92106ac5',
'unified:e2f5915d-cc6e-54eb-8cf9-f1ac1fd1ba34','bokheim:history-lcc:e739602c-c1b6-59cc-b594-d2759a263bf6',
'unified:8e12f4e1-058c-5a8c-bcb6-a65df85b172f','bokheim:lcc-detail:e8b0733e-757c-5a7f-9ff0-f691db9ee982',
'bokheim:lcc-detail:878d9585-639b-5a71-8f2c-66c48191e600','bokheim:france:french-revolution','bokheim:france:first-republic',
'bokheim:history-lcc-full:4a0acaa2-4d4b-5c70-8b69-cb830e7b1808','bokheim:lcc-detail:973fa435-545d-58cc-b49b-1f0d42553bce',
'bokheim:lcc-detail:7d3b2024-54c4-52b0-8962-d7d6a0296a0f','bokheim:history-lcc:267a50e3-ac40-54fe-b757-b38e4e43c3d5',
'bokheim:lcc-detail:ea2940eb-0f9e-54ca-9e23-9824e95763f0','bokheim:lcc-detail:2ed7ae64-1c47-5f80-b0b3-3d944240f22c',
'bokheim:history-lcc:282f1b24-40a0-5992-bf37-ccc03faaa3b4','bokheim:history-lcc:c559e7ac-482b-5400-84a6-e0b4b9d5aefd',
'bokheim:history-lcc:d2636f2a-236a-5b0c-8a1c-bf19c9519ea0','bokheim:history-lcc:47de3534-44ee-51c5-9805-37d13f02405d');

CREATE TEMP TABLE france_shared(concept_id TEXT PRIMARY KEY) WITHOUT ROWID;
WITH RECURSIVE shared(concept_id) AS (
 SELECT t.concept_id FROM france_tree t JOIN concept_parent p ON p.concept_id=t.concept_id
 WHERE p.parent_concept_id NOT IN (SELECT concept_id FROM france_tree)
 AND t.concept_id<>'unified:3754b57e-8d6a-5d89-b8bd-cedbb34ef143'
 UNION SELECT p.concept_id FROM shared s JOIN concept_parent p ON p.parent_concept_id=s.concept_id
 WHERE p.concept_id IN (SELECT concept_id FROM france_tree)
)
INSERT INTO france_shared SELECT concept_id FROM shared;

DELETE FROM concept_parent WHERE parent_concept_id IN (SELECT concept_id FROM france_tree);
DELETE FROM concept_parent WHERE concept_id IN (SELECT concept_id FROM france_tree WHERE concept_id NOT IN (SELECT concept_id FROM france_keep) AND concept_id NOT IN (SELECT concept_id FROM france_shared));
DELETE FROM concept WHERE concept_id IN (SELECT concept_id FROM france_tree WHERE concept_id NOT IN (SELECT concept_id FROM france_keep) AND concept_id NOT IN (SELECT concept_id FROM france_shared));

CREATE TEMP TABLE france_edge(child_id TEXT,parent_id TEXT,display_order INTEGER,PRIMARY KEY(child_id,parent_id)) WITHOUT ROWID;
INSERT INTO france_edge VALUES
('unified:743ac652-6402-5c66-b1ed-a3aff95fcb5a','unified:3754b57e-8d6a-5d89-b8bd-cedbb34ef143',10),
('bokheim:france:early-modern','unified:3754b57e-8d6a-5d89-b8bd-cedbb34ef143',20),
('bokheim:france:revolution-napoleon','unified:3754b57e-8d6a-5d89-b8bd-cedbb34ef143',30),
('bokheim:france:modern','unified:3754b57e-8d6a-5d89-b8bd-cedbb34ef143',40),
('bokheim:history-lcc-full:eabaaab4-cd7d-5dce-9990-79fcd8fb90fe','unified:743ac652-6402-5c66-b1ed-a3aff95fcb5a',10),
('bokheim:history-lcc-full:e751b1ad-86ff-54fb-9c28-bcdb92106ac5','unified:743ac652-6402-5c66-b1ed-a3aff95fcb5a',20),
('unified:e2f5915d-cc6e-54eb-8cf9-f1ac1fd1ba34','unified:743ac652-6402-5c66-b1ed-a3aff95fcb5a',30),
('bokheim:history-lcc:e739602c-c1b6-59cc-b594-d2759a263bf6','unified:743ac652-6402-5c66-b1ed-a3aff95fcb5a',40),
('unified:8e12f4e1-058c-5a8c-bcb6-a65df85b172f','bokheim:france:early-modern',10),
('bokheim:lcc-detail:e8b0733e-757c-5a7f-9ff0-f691db9ee982','bokheim:france:early-modern',20),
('bokheim:lcc-detail:878d9585-639b-5a71-8f2c-66c48191e600','bokheim:france:early-modern',30),
('bokheim:france:french-revolution','bokheim:france:revolution-napoleon',10),
('bokheim:france:first-republic','bokheim:france:revolution-napoleon',20),
('bokheim:history-lcc-full:4a0acaa2-4d4b-5c70-8b69-cb830e7b1808','bokheim:france:revolution-napoleon',30),
('bokheim:lcc-detail:973fa435-545d-58cc-b49b-1f0d42553bce','bokheim:france:modern',10),
('bokheim:lcc-detail:7d3b2024-54c4-52b0-8962-d7d6a0296a0f','bokheim:france:modern',20),
('bokheim:history-lcc:267a50e3-ac40-54fe-b757-b38e4e43c3d5','bokheim:france:modern',30),
('bokheim:lcc-detail:ea2940eb-0f9e-54ca-9e23-9824e95763f0','bokheim:france:modern',40),
('bokheim:lcc-detail:2ed7ae64-1c47-5f80-b0b3-3d944240f22c','bokheim:france:modern',50),
('bokheim:history-lcc:282f1b24-40a0-5992-bf37-ccc03faaa3b4','bokheim:france:modern',60),
('bokheim:history-lcc:c559e7ac-482b-5400-84a6-e0b4b9d5aefd','bokheim:france:modern',70),
('bokheim:history-lcc:d2636f2a-236a-5b0c-8a1c-bf19c9519ea0','bokheim:france:modern',80),
('bokheim:history-lcc:47de3534-44ee-51c5-9805-37d13f02405d','bokheim:france:modern',90);
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal,display_order)
SELECT e.child_id,e.parent_id,COALESCE((SELECT max(p.ordinal)+1 FROM concept_parent p WHERE p.concept_id=e.child_id),0),e.display_order FROM france_edge e;

UPDATE concept SET preferred_label='Early & Medieval' WHERE concept_id='unified:743ac652-6402-5c66-b1ed-a3aff95fcb5a';
UPDATE concept SET preferred_label='Late Medieval, 1328–1515' WHERE concept_id='unified:e2f5915d-cc6e-54eb-8cf9-f1ac1fd1ba34';
UPDATE concept SET preferred_label='Gaul & Celts' WHERE concept_id='bokheim:history-lcc-full:eabaaab4-cd7d-5dce-9990-79fcd8fb90fe';
UPDATE concept SET preferred_label='Frankish Period' WHERE concept_id='bokheim:history-lcc-full:e751b1ad-86ff-54fb-9c28-bcdb92106ac5';
UPDATE concept SET preferred_label='Hundred Years’ War, 1337–1453' WHERE concept_id='bokheim:history-lcc:e739602c-c1b6-59cc-b594-d2759a263bf6';
UPDATE concept SET preferred_label='16th Century' WHERE concept_id='unified:8e12f4e1-058c-5a8c-bcb6-a65df85b172f';
UPDATE concept SET preferred_label='1589–1715' WHERE concept_id='bokheim:lcc-detail:e8b0733e-757c-5a7f-9ff0-f691db9ee982';
UPDATE concept SET preferred_label='1715–1789' WHERE concept_id='bokheim:lcc-detail:878d9585-639b-5a71-8f2c-66c48191e600';
UPDATE concept SET preferred_label='First Empire, 1804–1815' WHERE concept_id='bokheim:history-lcc-full:4a0acaa2-4d4b-5c70-8b69-cb830e7b1808';
UPDATE concept SET preferred_label='Bourbon Restoration, 1815–1830' WHERE concept_id='bokheim:lcc-detail:973fa435-545d-58cc-b49b-1f0d42553bce';
UPDATE concept SET preferred_label='July Monarchy, 1830–1848' WHERE concept_id='bokheim:lcc-detail:7d3b2024-54c4-52b0-8962-d7d6a0296a0f';
UPDATE concept SET preferred_label='Second Republic, 1848–1851' WHERE concept_id='bokheim:history-lcc:267a50e3-ac40-54fe-b757-b38e4e43c3d5';
UPDATE concept SET preferred_label='Second Empire, 1852–1870' WHERE concept_id='bokheim:lcc-detail:ea2940eb-0f9e-54ca-9e23-9824e95763f0';
UPDATE concept SET preferred_label='Franco-Prussian War, 1870–1871' WHERE concept_id='bokheim:lcc-detail:2ed7ae64-1c47-5f80-b0b3-3d944240f22c';
UPDATE concept SET preferred_label='Third Republic, 1871–1940' WHERE concept_id='bokheim:history-lcc:282f1b24-40a0-5992-bf37-ccc03faaa3b4';
UPDATE concept SET preferred_label='Occupation & Liberation, 1940–1946' WHERE concept_id='bokheim:history-lcc:c559e7ac-482b-5400-84a6-e0b4b9d5aefd';
UPDATE concept SET preferred_label='Fourth Republic, 1946–1958' WHERE concept_id='bokheim:history-lcc:d2636f2a-236a-5b0c-8a1c-bf19c9519ea0';
UPDATE concept SET preferred_label='Fifth Republic, 1958–Present' WHERE concept_id='bokheim:history-lcc:47de3534-44ee-51c5-9805-37d13f02405d';

DROP TABLE france_edge; DROP TABLE france_shared; DROP TABLE france_keep; DROP TABLE france_tree;
COMMIT;

-- Curate Albania to a compact national chronology.
PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;
CREATE TEMP TABLE al_tree(concept_id TEXT PRIMARY KEY) WITHOUT ROWID;
WITH RECURSIVE t(id) AS (VALUES('unified:65646c99-df8a-5d5a-b671-ad20154e0fa8') UNION SELECT p.concept_id FROM t JOIN concept_parent p ON p.parent_concept_id=t.id) INSERT INTO al_tree SELECT id FROM t;

INSERT OR IGNORE INTO concept VALUES
('bokheim:history-lcc:e9c877f3-be95-5290-bf38-b1a1ffcbaed4','Medieval Albania, to 1501'),
('bokheim:history-lcc:9d298a8f-9adc-5744-b26a-c68d5cd59007','Ottoman Rule, 1501–1912'),
('bokheim:history-lcc-full:e22039f5-18d5-5560-ba00-b3c6c3c52d8a','National Awakening, 1840–1912'),
('bokheim:albania:early-independence','Early Independence, 1912–1939'),
('bokheim:history-lcc-full:d4f5587b-798d-59d1-8039-2aa54d1dc427','World War II, 1939–1944'),
('bokheim:history-lcc-full:68c89e2b-039e-5453-b59f-f77f55e86fdc','Communist Albania, 1944–1990'),
('bokheim:history-lcc-full:1aee304f-7b04-5d6b-92f3-907d79144157','Post-Communist Albania, 1990–Present');

-- Preserve general and dated political-history lookup without retaining wrappers.
INSERT OR IGNORE INTO source_selector(concept_id,system_id,selector,ordinal)
SELECT 'unified:65646c99-df8a-5d5a-b671-ad20154e0fa8',s.system_id,s.selector,COALESCE((SELECT max(x.ordinal)+1 FROM source_selector x WHERE x.concept_id='unified:65646c99-df8a-5d5a-b671-ad20154e0fa8' AND x.system_id=s.system_id),0)
FROM source_selector s WHERE s.concept_id='bokheim:history-lcc:932faed9-0a6a-57a4-98ab-03bf237aa2db';
INSERT OR IGNORE INTO source_selector(concept_id,system_id,selector,ordinal)
SELECT 'bokheim:history-lcc-full:1aee304f-7b04-5d6b-92f3-907d79144157',s.system_id,s.selector,COALESCE((SELECT max(x.ordinal)+1 FROM source_selector x WHERE x.concept_id='bokheim:history-lcc-full:1aee304f-7b04-5d6b-92f3-907d79144157' AND x.system_id=s.system_id),0)
FROM source_selector s WHERE s.concept_id='bokheim:history-lcc-full:28f0ee28-33fe-5912-9a14-632b34897a10';

CREATE TEMP TABLE al_keep(concept_id TEXT PRIMARY KEY) WITHOUT ROWID;
INSERT INTO al_keep VALUES
('unified:65646c99-df8a-5d5a-b671-ad20154e0fa8'),('bokheim:history-lcc:e9c877f3-be95-5290-bf38-b1a1ffcbaed4'),('bokheim:history-lcc:9d298a8f-9adc-5744-b26a-c68d5cd59007'),('bokheim:history-lcc-full:e22039f5-18d5-5560-ba00-b3c6c3c52d8a'),('bokheim:albania:early-independence'),('bokheim:history-lcc-full:d4f5587b-798d-59d1-8039-2aa54d1dc427'),('bokheim:history-lcc-full:68c89e2b-039e-5453-b59f-f77f55e86fdc'),('bokheim:history-lcc-full:1aee304f-7b04-5d6b-92f3-907d79144157');
CREATE TEMP TABLE al_shared(concept_id TEXT PRIMARY KEY) WITHOUT ROWID;
WITH RECURSIVE s(id) AS (SELECT t.concept_id FROM al_tree t JOIN concept_parent p ON p.concept_id=t.concept_id WHERE p.parent_concept_id NOT IN (SELECT concept_id FROM al_tree) AND t.concept_id<>'unified:65646c99-df8a-5d5a-b671-ad20154e0fa8' UNION SELECT p.concept_id FROM s JOIN concept_parent p ON p.parent_concept_id=s.id WHERE p.concept_id IN (SELECT concept_id FROM al_tree)) INSERT INTO al_shared SELECT id FROM s;
DELETE FROM concept_parent WHERE parent_concept_id IN (SELECT concept_id FROM al_tree);
DELETE FROM concept_parent WHERE concept_id IN (SELECT concept_id FROM al_tree WHERE concept_id NOT IN (SELECT concept_id FROM al_keep) AND concept_id NOT IN (SELECT concept_id FROM al_shared));
DELETE FROM concept WHERE concept_id IN (SELECT concept_id FROM al_tree WHERE concept_id NOT IN (SELECT concept_id FROM al_keep) AND concept_id NOT IN (SELECT concept_id FROM al_shared));

CREATE TEMP TABLE al_edge(child TEXT,parent TEXT,ord INTEGER,PRIMARY KEY(child,parent)) WITHOUT ROWID;
INSERT INTO al_edge VALUES
('bokheim:history-lcc:e9c877f3-be95-5290-bf38-b1a1ffcbaed4','unified:65646c99-df8a-5d5a-b671-ad20154e0fa8',10),
('bokheim:history-lcc:9d298a8f-9adc-5744-b26a-c68d5cd59007','unified:65646c99-df8a-5d5a-b671-ad20154e0fa8',20),
('bokheim:albania:early-independence','unified:65646c99-df8a-5d5a-b671-ad20154e0fa8',30),
('bokheim:history-lcc-full:d4f5587b-798d-59d1-8039-2aa54d1dc427','unified:65646c99-df8a-5d5a-b671-ad20154e0fa8',40),
('bokheim:history-lcc-full:68c89e2b-039e-5453-b59f-f77f55e86fdc','unified:65646c99-df8a-5d5a-b671-ad20154e0fa8',50),
('bokheim:history-lcc-full:1aee304f-7b04-5d6b-92f3-907d79144157','unified:65646c99-df8a-5d5a-b671-ad20154e0fa8',60),
('bokheim:history-lcc-full:e22039f5-18d5-5560-ba00-b3c6c3c52d8a','bokheim:history-lcc:9d298a8f-9adc-5744-b26a-c68d5cd59007',10);
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal,display_order) SELECT child,parent,COALESCE((SELECT max(ordinal)+1 FROM concept_parent WHERE concept_id=child),0),ord FROM al_edge;
UPDATE concept SET preferred_label=CASE concept_id WHEN 'bokheim:history-lcc:e9c877f3-be95-5290-bf38-b1a1ffcbaed4' THEN 'Medieval Albania, to 1501' WHEN 'bokheim:history-lcc:9d298a8f-9adc-5744-b26a-c68d5cd59007' THEN 'Ottoman Rule, 1501–1912' WHEN 'bokheim:history-lcc-full:e22039f5-18d5-5560-ba00-b3c6c3c52d8a' THEN 'National Awakening, 1840–1912' WHEN 'bokheim:history-lcc-full:d4f5587b-798d-59d1-8039-2aa54d1dc427' THEN 'World War II, 1939–1944' WHEN 'bokheim:history-lcc-full:68c89e2b-039e-5453-b59f-f77f55e86fdc' THEN 'Communist Albania, 1944–1990' WHEN 'bokheim:history-lcc-full:1aee304f-7b04-5d6b-92f3-907d79144157' THEN 'Post-Communist Albania, 1990–Present' ELSE preferred_label END WHERE concept_id IN (SELECT concept_id FROM al_keep);
DROP TABLE al_edge; DROP TABLE al_shared; DROP TABLE al_keep; DROP TABLE al_tree;
COMMIT;

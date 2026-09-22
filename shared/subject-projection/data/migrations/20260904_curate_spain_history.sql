-- Curate Spain into compact ancient, medieval, and modern period browsing.
PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;

CREATE TEMP TABLE spain_tree(concept_id TEXT PRIMARY KEY) WITHOUT ROWID;
WITH RECURSIVE tree(concept_id) AS (
 VALUES('unified:46059f35-84e2-546a-9b1d-b149357d5add')
 UNION SELECT p.concept_id FROM tree t JOIN concept_parent p ON p.parent_concept_id=t.concept_id
)
INSERT INTO spain_tree SELECT concept_id FROM tree;

INSERT OR IGNORE INTO concept(concept_id,preferred_label) VALUES
('bokheim:spain:al-andalus','Al-Andalus'),
('bokheim:spain:reconquista','Reconquista');

CREATE TEMP TABLE spain_keep(concept_id TEXT PRIMARY KEY) WITHOUT ROWID;
INSERT INTO spain_keep VALUES
('unified:46059f35-84e2-546a-9b1d-b149357d5add'),
('unified:491e648a-7d59-5e7c-a0dd-1d96aaeab1bb'),
('bokheim:history-lcc:1d14757d-395a-58ce-924c-bfec3ba68f79'),
('bokheim:history-lcc:1182fbe6-2702-5c60-b50e-5ea632c3d747'),
('bokheim:history-lcc:4bd6d124-0e76-50c8-b62c-7c8b84a76c59'),
('bokheim:history-lcc:40d4d8fb-71ce-5194-aa2b-3a6e571d0de4'),
('bokheim:spain:al-andalus'),('bokheim:spain:reconquista'),
('bokheim:history-lcc:b364e18e-0912-5b8e-858b-4e7bece0ac68'),
('bokheim:history-lcc:f6a55663-9915-53ee-b30f-146848e04b55'),
('bokheim:history-lcc:3bae2d24-a159-58e1-9bdc-dacf0f005b4d'),
('bokheim:history-lcc-full:bf57b9be-db07-53f4-9ffa-4001f6de865e'),
('unified:47d17d05-d6ba-5b65-ae19-4e4843b04f3a'),
('bokheim:history-lcc:a2dfeea6-b0bd-5abd-ac57-891c5ca21ba0'),
('bokheim:history-lcc:0f31dfb5-b511-513e-b41d-7eb11d306b35'),
('bokheim:history-lcc:05cabe50-db7c-5825-b44f-79d26c6a5373'),
('unified:ee98e6c6-188d-5966-af93-2832f4ddf81d'),
('bokheim:history-lcc:550c4be4-2503-5b9a-93ef-a09e05193cf0'),
('unified:e2e44309-506e-5eb9-8e69-46a8412d9e3d'),
('bokheim:history-lcc:a8617cd9-e891-5dd0-9d9d-5bb3bf07da56'),
('bokheim:history-lcc:df74c4f5-3f52-5025-92fd-ae658651c6fc'),
('bokheim:history-lcc:c19250d8-ed04-530f-96f1-132e682f7b77');

CREATE TEMP TABLE spain_shared(concept_id TEXT PRIMARY KEY) WITHOUT ROWID;
WITH RECURSIVE shared(concept_id) AS (
 SELECT t.concept_id FROM spain_tree t JOIN concept_parent p ON p.concept_id=t.concept_id
 WHERE p.parent_concept_id NOT IN (SELECT concept_id FROM spain_tree)
   AND t.concept_id<>'unified:46059f35-84e2-546a-9b1d-b149357d5add'
 UNION SELECT p.concept_id FROM shared s JOIN concept_parent p ON p.parent_concept_id=s.concept_id
 WHERE p.concept_id IN (SELECT concept_id FROM spain_tree)
)
INSERT INTO spain_shared SELECT concept_id FROM shared;

DELETE FROM concept_parent WHERE parent_concept_id IN (SELECT concept_id FROM spain_tree);
DELETE FROM concept_parent WHERE concept_id IN (
 SELECT concept_id FROM spain_tree WHERE concept_id NOT IN (SELECT concept_id FROM spain_keep) AND concept_id NOT IN (SELECT concept_id FROM spain_shared)
);
DELETE FROM concept WHERE concept_id IN (
 SELECT concept_id FROM spain_tree WHERE concept_id NOT IN (SELECT concept_id FROM spain_keep) AND concept_id NOT IN (SELECT concept_id FROM spain_shared)
);

CREATE TEMP TABLE spain_edge(child_id TEXT,parent_id TEXT,display_order INTEGER,PRIMARY KEY(child_id,parent_id)) WITHOUT ROWID;
INSERT INTO spain_edge VALUES
('unified:491e648a-7d59-5e7c-a0dd-1d96aaeab1bb','unified:46059f35-84e2-546a-9b1d-b149357d5add',10),
('bokheim:history-lcc:40d4d8fb-71ce-5194-aa2b-3a6e571d0de4','unified:46059f35-84e2-546a-9b1d-b149357d5add',20),
('unified:47d17d05-d6ba-5b65-ae19-4e4843b04f3a','unified:46059f35-84e2-546a-9b1d-b149357d5add',30),
('bokheim:history-lcc:1d14757d-395a-58ce-924c-bfec3ba68f79','unified:491e648a-7d59-5e7c-a0dd-1d96aaeab1bb',10),
('bokheim:history-lcc:1182fbe6-2702-5c60-b50e-5ea632c3d747','unified:491e648a-7d59-5e7c-a0dd-1d96aaeab1bb',20),
('bokheim:history-lcc:4bd6d124-0e76-50c8-b62c-7c8b84a76c59','unified:491e648a-7d59-5e7c-a0dd-1d96aaeab1bb',30),
('bokheim:spain:al-andalus','bokheim:history-lcc:40d4d8fb-71ce-5194-aa2b-3a6e571d0de4',10),
('bokheim:spain:reconquista','bokheim:history-lcc:40d4d8fb-71ce-5194-aa2b-3a6e571d0de4',20),
('bokheim:history-lcc:b364e18e-0912-5b8e-858b-4e7bece0ac68','bokheim:history-lcc:40d4d8fb-71ce-5194-aa2b-3a6e571d0de4',30),
('bokheim:history-lcc:f6a55663-9915-53ee-b30f-146848e04b55','bokheim:history-lcc:40d4d8fb-71ce-5194-aa2b-3a6e571d0de4',40),
('bokheim:history-lcc:3bae2d24-a159-58e1-9bdc-dacf0f005b4d','bokheim:history-lcc:40d4d8fb-71ce-5194-aa2b-3a6e571d0de4',50),
('bokheim:history-lcc-full:bf57b9be-db07-53f4-9ffa-4001f6de865e','bokheim:history-lcc:40d4d8fb-71ce-5194-aa2b-3a6e571d0de4',60),
('bokheim:history-lcc:a2dfeea6-b0bd-5abd-ac57-891c5ca21ba0','unified:47d17d05-d6ba-5b65-ae19-4e4843b04f3a',10),
('bokheim:history-lcc:0f31dfb5-b511-513e-b41d-7eb11d306b35','unified:47d17d05-d6ba-5b65-ae19-4e4843b04f3a',20),
('bokheim:history-lcc:05cabe50-db7c-5825-b44f-79d26c6a5373','unified:47d17d05-d6ba-5b65-ae19-4e4843b04f3a',30),
('unified:ee98e6c6-188d-5966-af93-2832f4ddf81d','unified:47d17d05-d6ba-5b65-ae19-4e4843b04f3a',40),
('bokheim:history-lcc:550c4be4-2503-5b9a-93ef-a09e05193cf0','unified:47d17d05-d6ba-5b65-ae19-4e4843b04f3a',50),
('unified:e2e44309-506e-5eb9-8e69-46a8412d9e3d','unified:47d17d05-d6ba-5b65-ae19-4e4843b04f3a',60),
('bokheim:history-lcc:a8617cd9-e891-5dd0-9d9d-5bb3bf07da56','unified:47d17d05-d6ba-5b65-ae19-4e4843b04f3a',70),
('bokheim:history-lcc:df74c4f5-3f52-5025-92fd-ae658651c6fc','unified:47d17d05-d6ba-5b65-ae19-4e4843b04f3a',80),
('bokheim:history-lcc:c19250d8-ed04-530f-96f1-132e682f7b77','unified:47d17d05-d6ba-5b65-ae19-4e4843b04f3a',90);
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal,display_order)
SELECT e.child_id,e.parent_id,COALESCE((SELECT max(p.ordinal)+1 FROM concept_parent p WHERE p.concept_id=e.child_id),0),e.display_order FROM spain_edge e;

UPDATE concept SET preferred_label='Ancient' WHERE concept_id='unified:491e648a-7d59-5e7c-a0dd-1d96aaeab1bb';
UPDATE concept SET preferred_label='Pre-Roman Spain' WHERE concept_id='bokheim:history-lcc:1d14757d-395a-58ce-924c-bfec3ba68f79';
UPDATE concept SET preferred_label='Roman Spain, 218 BC–414 AD' WHERE concept_id='bokheim:history-lcc:1182fbe6-2702-5c60-b50e-5ea632c3d747';
UPDATE concept SET preferred_label='Visigothic Spain, 414–711' WHERE concept_id='bokheim:history-lcc:4bd6d124-0e76-50c8-b62c-7c8b84a76c59';
UPDATE concept SET preferred_label='Medieval, 711–1516' WHERE concept_id='bokheim:history-lcc:40d4d8fb-71ce-5194-aa2b-3a6e571d0de4';
UPDATE concept SET preferred_label='Castile & León' WHERE concept_id='bokheim:history-lcc:b364e18e-0912-5b8e-858b-4e7bece0ac68';
UPDATE concept SET preferred_label='Aragon' WHERE concept_id='bokheim:history-lcc:f6a55663-9915-53ee-b30f-146848e04b55';
UPDATE concept SET preferred_label='Granada' WHERE concept_id='bokheim:history-lcc-full:bf57b9be-db07-53f4-9ffa-4001f6de865e';
UPDATE concept SET preferred_label='Modern, 1516–Present' WHERE concept_id='unified:47d17d05-d6ba-5b65-ae19-4e4843b04f3a';
UPDATE concept SET preferred_label='Habsburg Spain, 1516–1700' WHERE concept_id='bokheim:history-lcc:a2dfeea6-b0bd-5abd-ac57-891c5ca21ba0';
UPDATE concept SET preferred_label='Bourbon Spain, 1700–1808' WHERE concept_id='bokheim:history-lcc:0f31dfb5-b511-513e-b41d-7eb11d306b35';
UPDATE concept SET preferred_label='Napoleonic Period, 1808–1814' WHERE concept_id='bokheim:history-lcc:05cabe50-db7c-5825-b44f-79d26c6a5373';
UPDATE concept SET preferred_label='Bourbon Restoration, 1814–1868' WHERE concept_id='unified:ee98e6c6-188d-5966-af93-2832f4ddf81d';
UPDATE concept SET preferred_label='First Republic, 1873–1875' WHERE concept_id='bokheim:history-lcc:550c4be4-2503-5b9a-93ef-a09e05193cf0';
UPDATE concept SET preferred_label='Second Republic, 1931–1939' WHERE concept_id='unified:e2e44309-506e-5eb9-8e69-46a8412d9e3d';
UPDATE concept SET preferred_label='Spanish Civil War, 1936–1939' WHERE concept_id='bokheim:history-lcc:a8617cd9-e891-5dd0-9d9d-5bb3bf07da56';
UPDATE concept SET preferred_label='Francoist Spain, 1939–1975' WHERE concept_id='bokheim:history-lcc:df74c4f5-3f52-5025-92fd-ae658651c6fc';
UPDATE concept SET preferred_label='Democratic Spain, 1975–Present' WHERE concept_id='bokheim:history-lcc:c19250d8-ed04-530f-96f1-132e682f7b77';

DROP TABLE spain_edge; DROP TABLE spain_shared; DROP TABLE spain_keep; DROP TABLE spain_tree;
COMMIT;

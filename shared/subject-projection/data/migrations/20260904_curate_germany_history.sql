-- Curate Germany into compact chronological and major regional browsing.
-- The complete 605-node source branch remains in master-taxonomy-v2.sqlite3.
PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;

CREATE TEMP TABLE germany_tree(concept_id TEXT PRIMARY KEY) WITHOUT ROWID;
WITH RECURSIVE tree(concept_id) AS (
 VALUES('unified:0742de33-0fcd-55b4-9e2b-ea238f4f92bc')
 UNION SELECT p.concept_id FROM tree t JOIN concept_parent p ON p.parent_concept_id=t.concept_id
)
INSERT INTO germany_tree SELECT concept_id FROM tree;

-- Navigation-only period groups.
INSERT OR IGNORE INTO concept(concept_id,preferred_label) VALUES
('bokheim:germany:modern-history','Modern, 1815–Present'),
('bokheim:germany:reunified','Reunified Germany, 1990–Present');

-- Major regions imported as terminal curated concepts from the master graph.
INSERT OR IGNORE INTO concept(concept_id,preferred_label) VALUES
('bokheim:history-lcc-full:c2ff7367-7920-5fd6-9f96-b75117106fcd','Bavaria'),
('bokheim:history-lcc-full:4293fff8-b956-55f7-aa69-11f39395135b','Berlin'),
('bokheim:history-lcc-full:d282978a-6084-5065-bc2b-ea4881d5c7c3','Brandenburg'),
('bokheim:history-lcc-full:7a6c1484-8999-542b-9c3a-053603683462','Hanover'),
('bokheim:history-lcc-full:b9d94972-e747-514a-a4c3-758433df1d28','Rhineland'),
('bokheim:history-lcc-full:f87b6bbc-e0a8-5506-b7b5-8b0626e5b44e','Saxony'),
('bokheim:history-lcc-full:9f442029-059c-512c-b737-53f8b9d3b8e0','Westphalia'),
('bokheim:history-lcc-full:6ca8d0ce-79b7-553c-a231-f08d59513522','Württemberg');
INSERT OR IGNORE INTO concept(concept_id,preferred_label) VALUES
('bokheim:history-lcc-full:800d7dd7-bb75-5622-a30c-19d7679967d5','Hanseatic League');
INSERT OR IGNORE INTO source_selector(concept_id,system_id,selector,ordinal) VALUES
('bokheim:history-lcc-full:c2ff7367-7920-5fd6-9f96-b75117106fcd','lcc','DD801.B31..DD801.43@6',0),
('bokheim:history-lcc-full:4293fff8-b956-55f7-aa69-11f39395135b','lcc','DD851..DD900@5',0),
('bokheim:history-lcc-full:d282978a-6084-5065-bc2b-ea4881d5c7c3','lcc','DD801.B68..DD801.B689@6',0),
('bokheim:history-lcc-full:7a6c1484-8999-542b-9c3a-053603683462','lcc','DD801.H164..DD801.H1649@6',0),
('bokheim:history-lcc-full:b9d94972-e747-514a-a4c3-758433df1d28','lcc','DD801.R682..DD801.R682@6',0),
('bokheim:history-lcc-full:f87b6bbc-e0a8-5506-b7b5-8b0626e5b44e','lcc','DD801.S31..DD801.S46@6',0),
('bokheim:history-lcc-full:9f442029-059c-512c-b737-53f8b9d3b8e0','lcc','DD801.W54..DD801.W549@6',0),
('bokheim:history-lcc-full:6ca8d0ce-79b7-553c-a231-f08d59513522','lcc','DD801.W6..DD801.W82@6',0);
INSERT OR IGNORE INTO source_selector(concept_id,system_id,selector,ordinal) VALUES
('bokheim:history-lcc-full:800d7dd7-bb75-5622-a30c-19d7679967d5','lcc','DD801.H17..DD801.H25@6',0);

CREATE TEMP TABLE germany_keep(concept_id TEXT PRIMARY KEY) WITHOUT ROWID;
INSERT INTO germany_keep VALUES
('unified:0742de33-0fcd-55b4-9e2b-ea238f4f92bc'),
('bokheim:history-lcc:2ef1949a-b47b-5da1-bbdf-2b7ecddc1934'),
('unified:0e54a4c2-d6cf-55d1-8221-c1f6a9c26b87'),
('bokheim:history-lcc:d079130c-7508-57bb-871b-88fa83e6a093'),
('bokheim:history-lcc:dbd53e25-dd15-5619-8530-92249d40b34f'),
('bokheim:history-lcc:2a8d1320-8b85-57ae-a2c2-74c7720f94ff'),
('bokheim:history-lcc:47d05e4d-c2a3-5c31-8481-a47bdf0e8394'),
('unified:9cb01fe8-95c5-592e-9a2a-cd17799d7cf3'),
('unified:01395d8a-c308-5246-8f5c-1bdc8a30a6ce'),
('bokheim:history-lcc:ddcbca8d-cb42-59bb-8499-73ec7399896e'),
('bokheim:history-lcc:5d898107-0247-56de-b629-7fdef62f91e6'),
('bokheim:history-lcc:a00b26d1-e930-5ca9-9ee8-bd0e50903ac9'),
('bokheim:history-lcc:719548f8-7887-58b3-9f72-65182939d301'),
('bokheim:germany:modern-history'),
('bokheim:history-lcc:e03cb959-20e1-58b2-8c57-449a5414fd8a'),
('unified:cf4234dc-b4a9-59f8-a933-3a182d4da3f1'),
('unified:4ca8c989-e348-58ff-9187-2cd01f579fc8'),
('unified:9c4ae38a-47ed-5e4a-9457-5fba75f7d7f6'),
('unified:80db55a5-58f7-54c0-94f0-c1da4131590d'),
('unified:0fba62ad-4128-5a39-a98d-eec60dd021e1'),
('bokheim:germany:reunified'),
('unified:f6f4a02c-f7b2-59c1-9ca9-63136b8eceac'),
('bokheim:history-lcc-full:a5528b81-1700-57e4-8ef1-5d076c073f51'),
('bokheim:history-lcc-full:c2ff7367-7920-5fd6-9f96-b75117106fcd'),
('bokheim:history-lcc-full:4293fff8-b956-55f7-aa69-11f39395135b'),
('bokheim:history-lcc-full:d282978a-6084-5065-bc2b-ea4881d5c7c3'),
('bokheim:history-lcc-full:7a6c1484-8999-542b-9c3a-053603683462'),
('bokheim:history-lcc-full:b9d94972-e747-514a-a4c3-758433df1d28'),
('bokheim:history-lcc-full:f87b6bbc-e0a8-5506-b7b5-8b0626e5b44e'),
('bokheim:history-lcc-full:9f442029-059c-512c-b737-53f8b9d3b8e0'),
('bokheim:history-lcc-full:6ca8d0ce-79b7-553c-a231-f08d59513522');
INSERT OR IGNORE INTO germany_keep VALUES ('bokheim:history-lcc-full:800d7dd7-bb75-5622-a30c-19d7679967d5');

-- Preserve external routes to shared concepts before replacing Germany's routes.
CREATE TEMP TABLE germany_shared(concept_id TEXT PRIMARY KEY) WITHOUT ROWID;
WITH RECURSIVE shared(concept_id) AS (
 SELECT t.concept_id FROM germany_tree t JOIN concept_parent p ON p.concept_id=t.concept_id
 WHERE p.parent_concept_id NOT IN (SELECT concept_id FROM germany_tree)
   AND t.concept_id<>'unified:0742de33-0fcd-55b4-9e2b-ea238f4f92bc'
 UNION SELECT p.concept_id FROM shared s JOIN concept_parent p ON p.parent_concept_id=s.concept_id
 WHERE p.concept_id IN (SELECT concept_id FROM germany_tree)
)
INSERT INTO germany_shared SELECT concept_id FROM shared;

DELETE FROM concept_parent WHERE parent_concept_id IN (SELECT concept_id FROM germany_tree);
DELETE FROM concept_parent
WHERE concept_id IN (SELECT concept_id FROM germany_tree WHERE concept_id NOT IN (SELECT concept_id FROM germany_keep) AND concept_id NOT IN (SELECT concept_id FROM germany_shared));
DELETE FROM concept
WHERE concept_id IN (SELECT concept_id FROM germany_tree WHERE concept_id NOT IN (SELECT concept_id FROM germany_keep) AND concept_id NOT IN (SELECT concept_id FROM germany_shared));

-- Rebuild the compact hierarchy.
CREATE TEMP TABLE germany_edge(child_id TEXT,parent_id TEXT,display_order INTEGER,PRIMARY KEY(child_id,parent_id)) WITHOUT ROWID;
INSERT INTO germany_edge VALUES
('bokheim:history-lcc:2ef1949a-b47b-5da1-bbdf-2b7ecddc1934','unified:0742de33-0fcd-55b4-9e2b-ea238f4f92bc',10),
('unified:0e54a4c2-d6cf-55d1-8221-c1f6a9c26b87','unified:0742de33-0fcd-55b4-9e2b-ea238f4f92bc',20),
('unified:9cb01fe8-95c5-592e-9a2a-cd17799d7cf3','unified:0742de33-0fcd-55b4-9e2b-ea238f4f92bc',30),
('bokheim:germany:modern-history','unified:0742de33-0fcd-55b4-9e2b-ea238f4f92bc',40),
('unified:f6f4a02c-f7b2-59c1-9ca9-63136b8eceac','unified:0742de33-0fcd-55b4-9e2b-ea238f4f92bc',50),
('bokheim:history-lcc-full:800d7dd7-bb75-5622-a30c-19d7679967d5','unified:0742de33-0fcd-55b4-9e2b-ea238f4f92bc',60),
('bokheim:history-lcc:d079130c-7508-57bb-871b-88fa83e6a093','unified:0e54a4c2-d6cf-55d1-8221-c1f6a9c26b87',10),
('bokheim:history-lcc:dbd53e25-dd15-5619-8530-92249d40b34f','unified:0e54a4c2-d6cf-55d1-8221-c1f6a9c26b87',20),
('bokheim:history-lcc:2a8d1320-8b85-57ae-a2c2-74c7720f94ff','unified:0e54a4c2-d6cf-55d1-8221-c1f6a9c26b87',30),
('bokheim:history-lcc:47d05e4d-c2a3-5c31-8481-a47bdf0e8394','unified:0e54a4c2-d6cf-55d1-8221-c1f6a9c26b87',40),
('unified:01395d8a-c308-5246-8f5c-1bdc8a30a6ce','unified:9cb01fe8-95c5-592e-9a2a-cd17799d7cf3',10),
('bokheim:history-lcc:ddcbca8d-cb42-59bb-8499-73ec7399896e','unified:9cb01fe8-95c5-592e-9a2a-cd17799d7cf3',20),
('bokheim:history-lcc:5d898107-0247-56de-b629-7fdef62f91e6','unified:9cb01fe8-95c5-592e-9a2a-cd17799d7cf3',30),
('bokheim:history-lcc:a00b26d1-e930-5ca9-9ee8-bd0e50903ac9','unified:9cb01fe8-95c5-592e-9a2a-cd17799d7cf3',40),
('bokheim:history-lcc:719548f8-7887-58b3-9f72-65182939d301','unified:9cb01fe8-95c5-592e-9a2a-cd17799d7cf3',50),
('unified:cf4234dc-b4a9-59f8-a933-3a182d4da3f1','bokheim:germany:modern-history',10),
('bokheim:history-lcc:e03cb959-20e1-58b2-8c57-449a5414fd8a','bokheim:germany:modern-history',5),
('unified:4ca8c989-e348-58ff-9187-2cd01f579fc8','bokheim:germany:modern-history',20),
('unified:9c4ae38a-47ed-5e4a-9457-5fba75f7d7f6','bokheim:germany:modern-history',30),
('unified:80db55a5-58f7-54c0-94f0-c1da4131590d','bokheim:germany:modern-history',40),
('unified:0fba62ad-4128-5a39-a98d-eec60dd021e1','bokheim:germany:modern-history',50),
('bokheim:germany:reunified','bokheim:germany:modern-history',60),
('bokheim:history-lcc-full:a5528b81-1700-57e4-8ef1-5d076c073f51','unified:f6f4a02c-f7b2-59c1-9ca9-63136b8eceac',10);

INSERT INTO germany_edge
SELECT concept_id,'unified:0742de33-0fcd-55b4-9e2b-ea238f4f92bc',100+row_number() OVER (ORDER BY preferred_label)*10
FROM concept WHERE concept_id IN (
'bokheim:history-lcc-full:c2ff7367-7920-5fd6-9f96-b75117106fcd','bokheim:history-lcc-full:4293fff8-b956-55f7-aa69-11f39395135b',
'bokheim:history-lcc-full:d282978a-6084-5065-bc2b-ea4881d5c7c3','bokheim:history-lcc-full:7a6c1484-8999-542b-9c3a-053603683462',
'bokheim:history-lcc-full:b9d94972-e747-514a-a4c3-758433df1d28','bokheim:history-lcc-full:f87b6bbc-e0a8-5506-b7b5-8b0626e5b44e',
'bokheim:history-lcc-full:9f442029-059c-512c-b737-53f8b9d3b8e0','bokheim:history-lcc-full:6ca8d0ce-79b7-553c-a231-f08d59513522');

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal,display_order)
SELECT e.child_id,e.parent_id,COALESCE((SELECT max(p.ordinal)+1 FROM concept_parent p WHERE p.concept_id=e.child_id),0),e.display_order FROM germany_edge e;

UPDATE concept SET preferred_label='Early Germanic, to 481' WHERE concept_id='bokheim:history-lcc:2ef1949a-b47b-5da1-bbdf-2b7ecddc1934';
UPDATE concept SET preferred_label='Early & Medieval, 481–1519' WHERE concept_id='unified:0e54a4c2-d6cf-55d1-8221-c1f6a9c26b87';
UPDATE concept SET preferred_label='Early Modern, 1519–1815' WHERE concept_id='unified:9cb01fe8-95c5-592e-9a2a-cd17799d7cf3';
UPDATE concept SET preferred_label='Reformation, 1519–1648' WHERE concept_id='unified:01395d8a-c308-5246-8f5c-1bdc8a30a6ce';
UPDATE concept SET preferred_label='Thirty Years’ War, 1618–1648' WHERE concept_id='bokheim:history-lcc:ddcbca8d-cb42-59bb-8499-73ec7399896e';
UPDATE concept SET preferred_label='German Empire, 1871–1918' WHERE concept_id='unified:cf4234dc-b4a9-59f8-a933-3a182d4da3f1';
UPDATE concept SET preferred_label='German Unification, 1815–1871' WHERE concept_id='bokheim:history-lcc:e03cb959-20e1-58b2-8c57-449a5414fd8a';
UPDATE concept SET preferred_label='Weimar Republic, 1918–1933' WHERE concept_id='unified:4ca8c989-e348-58ff-9187-2cd01f579fc8';
UPDATE concept SET preferred_label='Nazi Germany, 1933–1945' WHERE concept_id='unified:9c4ae38a-47ed-5e4a-9457-5fba75f7d7f6';
UPDATE concept SET preferred_label='East Germany, 1949–1990' WHERE concept_id='unified:80db55a5-58f7-54c0-94f0-c1da4131590d';
UPDATE concept SET preferred_label='West Germany, 1949–1990' WHERE concept_id='unified:0fba62ad-4128-5a39-a98d-eec60dd021e1';
UPDATE concept SET preferred_label='Hohenzollern Dynasty, 1415–1640' WHERE concept_id='bokheim:history-lcc-full:a5528b81-1700-57e4-8ef1-5d076c073f51';

DROP TABLE germany_edge; DROP TABLE germany_shared; DROP TABLE germany_keep; DROP TABLE germany_tree;
COMMIT;

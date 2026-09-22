-- Curate Portugal into a compact chronological tree.
PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;
CREATE TEMP TABLE pt_tree(concept_id TEXT PRIMARY KEY) WITHOUT ROWID;
WITH RECURSIVE t(id) AS (VALUES('unified:bcf05a2d-7986-5262-9c58-af0dcea68f64') UNION SELECT p.concept_id FROM t JOIN concept_parent p ON p.parent_concept_id=t.id) INSERT INTO pt_tree SELECT id FROM t;

INSERT OR IGNORE INTO concept VALUES
('bokheim:history-lcc-full:1fdc2546-0424-5027-a0b2-e349b3166720','Early Portugal, to 1095'),
('unified:ca1ef7a9-10b9-5dbf-929c-0d21b3cf6232','Medieval Portugal, 1095–1580'),
('bokheim:history-lcc:a8e4f8c0-0998-5f55-b8d6-e26332d8f8a3','House of Burgundy, 1095–1383'),
('bokheim:history-lcc:0c579ccd-127c-5fb7-8239-d06833e69157','Interregnum, 1383–1385'),
('bokheim:history-lcc:aea37a38-cec1-54e5-8daa-bc3181c38c20','House of Aviz, 1385–1580'),
('bokheim:history-lcc:2bb8c8d5-2c97-52a8-a5a5-bff41ba94ad1','Iberian Union, 1580–1640'),
('bokheim:portugal:braganza','House of Braganza, 1640–1910'),
('bokheim:portugal:constitutional-revolution','Constitutional Revolution, 1820'),
('bokheim:portugal:constitutional-monarchy','Constitutional Monarchy, 1820–1910'),
('bokheim:portugal:first-republic','First Republic, 1910–1926'),
('bokheim:history-lcc-full:ddb024cf-57e9-5885-a153-536dab038be2','World War I, 1914–1918'),
('bokheim:portugal:military-dictatorship','Military Dictatorship, 1926–1933'),
('bokheim:portugal:estado-novo','Estado Novo, 1933–1974'),
('bokheim:history-lcc-full:e13e04d9-30d6-50e7-8ea6-85aed98ab514','Democratic Portugal, 1974–Present');

CREATE TEMP TABLE pt_keep(concept_id TEXT PRIMARY KEY) WITHOUT ROWID;
INSERT INTO pt_keep SELECT concept_id FROM concept WHERE concept_id='unified:bcf05a2d-7986-5262-9c58-af0dcea68f64' OR concept_id LIKE 'bokheim:portugal:%' OR concept_id IN
('bokheim:history-lcc-full:1fdc2546-0424-5027-a0b2-e349b3166720','unified:ca1ef7a9-10b9-5dbf-929c-0d21b3cf6232','bokheim:history-lcc:a8e4f8c0-0998-5f55-b8d6-e26332d8f8a3','bokheim:history-lcc:0c579ccd-127c-5fb7-8239-d06833e69157','bokheim:history-lcc:aea37a38-cec1-54e5-8daa-bc3181c38c20','bokheim:history-lcc:2bb8c8d5-2c97-52a8-a5a5-bff41ba94ad1','bokheim:history-lcc-full:ddb024cf-57e9-5885-a153-536dab038be2','bokheim:history-lcc-full:e13e04d9-30d6-50e7-8ea6-85aed98ab514');

CREATE TEMP TABLE pt_shared(concept_id TEXT PRIMARY KEY) WITHOUT ROWID;
WITH RECURSIVE s(id) AS (SELECT t.concept_id FROM pt_tree t JOIN concept_parent p ON p.concept_id=t.concept_id WHERE p.parent_concept_id NOT IN (SELECT concept_id FROM pt_tree) AND t.concept_id<>'unified:bcf05a2d-7986-5262-9c58-af0dcea68f64' UNION SELECT p.concept_id FROM s JOIN concept_parent p ON p.parent_concept_id=s.id WHERE p.concept_id IN (SELECT concept_id FROM pt_tree)) INSERT INTO pt_shared SELECT id FROM s;
DELETE FROM concept_parent WHERE parent_concept_id IN (SELECT concept_id FROM pt_tree);
DELETE FROM concept_parent WHERE concept_id IN (SELECT concept_id FROM pt_tree WHERE concept_id NOT IN (SELECT concept_id FROM pt_keep) AND concept_id NOT IN (SELECT concept_id FROM pt_shared));
DELETE FROM concept WHERE concept_id IN (SELECT concept_id FROM pt_tree WHERE concept_id NOT IN (SELECT concept_id FROM pt_keep) AND concept_id NOT IN (SELECT concept_id FROM pt_shared));

CREATE TEMP TABLE pt_edge(child TEXT,parent TEXT,ord INTEGER,PRIMARY KEY(child,parent)) WITHOUT ROWID;
INSERT INTO pt_edge VALUES
('bokheim:history-lcc-full:1fdc2546-0424-5027-a0b2-e349b3166720','unified:bcf05a2d-7986-5262-9c58-af0dcea68f64',10),
('unified:ca1ef7a9-10b9-5dbf-929c-0d21b3cf6232','unified:bcf05a2d-7986-5262-9c58-af0dcea68f64',20),
('bokheim:history-lcc:2bb8c8d5-2c97-52a8-a5a5-bff41ba94ad1','unified:bcf05a2d-7986-5262-9c58-af0dcea68f64',30),
('bokheim:portugal:braganza','unified:bcf05a2d-7986-5262-9c58-af0dcea68f64',40),
('bokheim:portugal:first-republic','unified:bcf05a2d-7986-5262-9c58-af0dcea68f64',50),
('bokheim:portugal:military-dictatorship','unified:bcf05a2d-7986-5262-9c58-af0dcea68f64',60),
('bokheim:portugal:estado-novo','unified:bcf05a2d-7986-5262-9c58-af0dcea68f64',70),
('bokheim:history-lcc-full:e13e04d9-30d6-50e7-8ea6-85aed98ab514','unified:bcf05a2d-7986-5262-9c58-af0dcea68f64',80),
('bokheim:history-lcc:a8e4f8c0-0998-5f55-b8d6-e26332d8f8a3','unified:ca1ef7a9-10b9-5dbf-929c-0d21b3cf6232',10),
('bokheim:history-lcc:0c579ccd-127c-5fb7-8239-d06833e69157','unified:ca1ef7a9-10b9-5dbf-929c-0d21b3cf6232',20),
('bokheim:history-lcc:aea37a38-cec1-54e5-8daa-bc3181c38c20','unified:ca1ef7a9-10b9-5dbf-929c-0d21b3cf6232',30),
('bokheim:portugal:constitutional-revolution','bokheim:portugal:braganza',10),
('bokheim:portugal:constitutional-monarchy','bokheim:portugal:braganza',20),
('bokheim:history-lcc-full:ddb024cf-57e9-5885-a153-536dab038be2','bokheim:portugal:first-republic',10);
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal,display_order) SELECT child,parent,COALESCE((SELECT max(ordinal)+1 FROM concept_parent WHERE concept_id=child),0),ord FROM pt_edge;

UPDATE concept SET preferred_label=CASE concept_id
WHEN 'bokheim:history-lcc-full:1fdc2546-0424-5027-a0b2-e349b3166720' THEN 'Early Portugal, to 1095'
WHEN 'unified:ca1ef7a9-10b9-5dbf-929c-0d21b3cf6232' THEN 'Medieval Portugal, 1095–1580'
WHEN 'bokheim:history-lcc:a8e4f8c0-0998-5f55-b8d6-e26332d8f8a3' THEN 'House of Burgundy, 1095–1383'
WHEN 'bokheim:history-lcc:0c579ccd-127c-5fb7-8239-d06833e69157' THEN 'Interregnum, 1383–1385'
WHEN 'bokheim:history-lcc:aea37a38-cec1-54e5-8daa-bc3181c38c20' THEN 'House of Aviz, 1385–1580'
WHEN 'bokheim:history-lcc:2bb8c8d5-2c97-52a8-a5a5-bff41ba94ad1' THEN 'Iberian Union, 1580–1640'
WHEN 'bokheim:history-lcc-full:ddb024cf-57e9-5885-a153-536dab038be2' THEN 'World War I, 1914–1918'
WHEN 'bokheim:history-lcc-full:e13e04d9-30d6-50e7-8ea6-85aed98ab514' THEN 'Democratic Portugal, 1974–Present'
ELSE preferred_label END WHERE concept_id IN (SELECT concept_id FROM pt_keep);
DROP TABLE pt_edge; DROP TABLE pt_shared; DROP TABLE pt_keep; DROP TABLE pt_tree;
COMMIT;

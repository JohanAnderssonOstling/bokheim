-- Curate Switzerland into national periods; retain generic political-history lookup at root.
PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;
CREATE TEMP TABLE ch_tree(concept_id TEXT PRIMARY KEY) WITHOUT ROWID;
WITH RECURSIVE t(id) AS (VALUES('unified:ea4b918f-9a52-516f-9bff-e2d73503516e') UNION SELECT p.concept_id FROM t JOIN concept_parent p ON p.parent_concept_id=t.id) INSERT INTO ch_tree SELECT id FROM t;

-- A general Swiss political-history classification resolves to Switzerland itself.
INSERT OR IGNORE INTO source_selector(concept_id,system_id,selector,ordinal)
SELECT 'unified:ea4b918f-9a52-516f-9bff-e2d73503516e',s.system_id,s.selector,
 COALESCE((SELECT max(x.ordinal)+1 FROM source_selector x WHERE x.concept_id='unified:ea4b918f-9a52-516f-9bff-e2d73503516e' AND x.system_id=s.system_id),0)
FROM source_selector s WHERE s.concept_id='bokheim:lcc-detail:4db5eecb-6349-5c01-b3e8-0b17c4fa1d53';

INSERT OR IGNORE INTO concept VALUES
('bokheim:switzerland:ancient-medieval','Ancient & Early Medieval'),
('bokheim:history-lcc:0b6f429c-1053-5b65-8748-593091571ccf','Celtic & Roman Switzerland, to 406'),
('bokheim:history-lcc:412278d0-af21-593f-990c-33d6161029df','Germanic Period, 406–687'),
('bokheim:history-lcc:acf0977d-045c-5d24-9a86-f2a5337a9f22','Carolingian & Imperial Rule, 687–1291'),
('bokheim:history-lcc:3c17b101-5cb2-5c2a-8632-b8889edea122','Old Swiss Confederacy, 1291–1516'),
('bokheim:history-lcc:0b0279b2-146a-5dec-9d29-e0b7c99c6d7d','Early Modern Confederacy, 1516–1798'),
('bokheim:history-lcc-full:2a4e3bf1-c522-5d5d-8d2f-c099b1d7ae3e','Reformation & Consolidation, 1516–1648'),
('bokheim:switzerland:ancien-regime','Ancien Régime, 1648–1798'),
('bokheim:history-lcc:ca188862-c9b8-58ce-8404-04485152ff36','Revolutionary & Napoleonic Era, 1789–1815'),
('bokheim:history-lcc-full:4d670003-87e0-51f1-b402-3e46d931d595','Helvetic Republic, 1798–1803'),
('bokheim:switzerland:mediation','Mediation Period, 1803–1815'),
('bokheim:history-lcc:af2b8c64-64a3-539d-8ad4-d1173f4385fe','Restoration, 1815–1830'),
('bokheim:history-lcc:f0ed96d0-6391-5dc9-a74e-bc7125f6b155','Regeneration, 1830–1848'),
('bokheim:history-lcc-full:dc286a7d-5109-5f79-9e8a-5294ca872cae','Sonderbund War, 1847'),
('bokheim:switzerland:federal','Federal Switzerland, 1848–Present'),
('bokheim:history-lcc:9e6c42c0-7633-5e96-b380-6f943b152fa2','Early Federal State, 1848–1900'),
('bokheim:history-lcc-full:0f53ab0d-15b9-54c2-bf64-043414c679b9','World Wars Era, 1900–1945'),
('bokheim:history-lcc-full:45f217da-3e49-55b4-8a88-89c2cbc5c326','Postwar Switzerland, 1945–2000'),
('bokheim:history-lcc:189714c8-464a-5e5b-9bb2-8dd50b08b43f','Contemporary Switzerland, 2000–Present');

CREATE TEMP TABLE ch_keep(concept_id TEXT PRIMARY KEY) WITHOUT ROWID;
INSERT INTO ch_keep SELECT concept_id FROM concept WHERE concept_id='unified:ea4b918f-9a52-516f-9bff-e2d73503516e' OR concept_id LIKE 'bokheim:switzerland:%' OR concept_id IN
('bokheim:history-lcc:0b6f429c-1053-5b65-8748-593091571ccf','bokheim:history-lcc:412278d0-af21-593f-990c-33d6161029df','bokheim:history-lcc:acf0977d-045c-5d24-9a86-f2a5337a9f22','bokheim:history-lcc:3c17b101-5cb2-5c2a-8632-b8889edea122','bokheim:history-lcc:0b0279b2-146a-5dec-9d29-e0b7c99c6d7d','bokheim:history-lcc-full:2a4e3bf1-c522-5d5d-8d2f-c099b1d7ae3e','bokheim:history-lcc:ca188862-c9b8-58ce-8404-04485152ff36','bokheim:history-lcc-full:4d670003-87e0-51f1-b402-3e46d931d595','bokheim:history-lcc:af2b8c64-64a3-539d-8ad4-d1173f4385fe','bokheim:history-lcc:f0ed96d0-6391-5dc9-a74e-bc7125f6b155','bokheim:history-lcc-full:dc286a7d-5109-5f79-9e8a-5294ca872cae','bokheim:history-lcc:9e6c42c0-7633-5e96-b380-6f943b152fa2','bokheim:history-lcc-full:0f53ab0d-15b9-54c2-bf64-043414c679b9','bokheim:history-lcc-full:45f217da-3e49-55b4-8a88-89c2cbc5c326','bokheim:history-lcc:189714c8-464a-5e5b-9bb2-8dd50b08b43f');

CREATE TEMP TABLE ch_shared(concept_id TEXT PRIMARY KEY) WITHOUT ROWID;
WITH RECURSIVE s(id) AS (SELECT t.concept_id FROM ch_tree t JOIN concept_parent p ON p.concept_id=t.concept_id WHERE p.parent_concept_id NOT IN (SELECT concept_id FROM ch_tree) AND t.concept_id<>'unified:ea4b918f-9a52-516f-9bff-e2d73503516e' UNION SELECT p.concept_id FROM s JOIN concept_parent p ON p.parent_concept_id=s.id WHERE p.concept_id IN (SELECT concept_id FROM ch_tree)) INSERT INTO ch_shared SELECT id FROM s;
DELETE FROM concept_parent WHERE parent_concept_id IN (SELECT concept_id FROM ch_tree);
DELETE FROM concept_parent WHERE concept_id IN (SELECT concept_id FROM ch_tree WHERE concept_id NOT IN (SELECT concept_id FROM ch_keep) AND concept_id NOT IN (SELECT concept_id FROM ch_shared));
DELETE FROM concept WHERE concept_id IN (SELECT concept_id FROM ch_tree WHERE concept_id NOT IN (SELECT concept_id FROM ch_keep) AND concept_id NOT IN (SELECT concept_id FROM ch_shared));

CREATE TEMP TABLE ch_edge(child TEXT,parent TEXT,ord INTEGER,PRIMARY KEY(child,parent)) WITHOUT ROWID;
INSERT INTO ch_edge VALUES
('bokheim:switzerland:ancient-medieval','unified:ea4b918f-9a52-516f-9bff-e2d73503516e',10),('bokheim:history-lcc:3c17b101-5cb2-5c2a-8632-b8889edea122','unified:ea4b918f-9a52-516f-9bff-e2d73503516e',20),('bokheim:history-lcc:0b0279b2-146a-5dec-9d29-e0b7c99c6d7d','unified:ea4b918f-9a52-516f-9bff-e2d73503516e',30),('bokheim:history-lcc:ca188862-c9b8-58ce-8404-04485152ff36','unified:ea4b918f-9a52-516f-9bff-e2d73503516e',40),('bokheim:history-lcc:af2b8c64-64a3-539d-8ad4-d1173f4385fe','unified:ea4b918f-9a52-516f-9bff-e2d73503516e',50),('bokheim:history-lcc:f0ed96d0-6391-5dc9-a74e-bc7125f6b155','unified:ea4b918f-9a52-516f-9bff-e2d73503516e',60),('bokheim:switzerland:federal','unified:ea4b918f-9a52-516f-9bff-e2d73503516e',70),
('bokheim:history-lcc:0b6f429c-1053-5b65-8748-593091571ccf','bokheim:switzerland:ancient-medieval',10),('bokheim:history-lcc:412278d0-af21-593f-990c-33d6161029df','bokheim:switzerland:ancient-medieval',20),('bokheim:history-lcc:acf0977d-045c-5d24-9a86-f2a5337a9f22','bokheim:switzerland:ancient-medieval',30),
('bokheim:history-lcc-full:2a4e3bf1-c522-5d5d-8d2f-c099b1d7ae3e','bokheim:history-lcc:0b0279b2-146a-5dec-9d29-e0b7c99c6d7d',10),('bokheim:switzerland:ancien-regime','bokheim:history-lcc:0b0279b2-146a-5dec-9d29-e0b7c99c6d7d',20),
('bokheim:history-lcc-full:4d670003-87e0-51f1-b402-3e46d931d595','bokheim:history-lcc:ca188862-c9b8-58ce-8404-04485152ff36',10),('bokheim:switzerland:mediation','bokheim:history-lcc:ca188862-c9b8-58ce-8404-04485152ff36',20),
('bokheim:history-lcc-full:dc286a7d-5109-5f79-9e8a-5294ca872cae','bokheim:history-lcc:f0ed96d0-6391-5dc9-a74e-bc7125f6b155',10),
('bokheim:history-lcc:9e6c42c0-7633-5e96-b380-6f943b152fa2','bokheim:switzerland:federal',10),('bokheim:history-lcc-full:0f53ab0d-15b9-54c2-bf64-043414c679b9','bokheim:switzerland:federal',20),('bokheim:history-lcc-full:45f217da-3e49-55b4-8a88-89c2cbc5c326','bokheim:switzerland:federal',30),('bokheim:history-lcc:189714c8-464a-5e5b-9bb2-8dd50b08b43f','bokheim:switzerland:federal',40);
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal,display_order) SELECT child,parent,COALESCE((SELECT max(ordinal)+1 FROM concept_parent WHERE concept_id=child),0),ord FROM ch_edge;

UPDATE concept SET preferred_label=CASE concept_id WHEN 'bokheim:history-lcc:0b6f429c-1053-5b65-8748-593091571ccf' THEN 'Celtic & Roman Switzerland, to 406' WHEN 'bokheim:history-lcc:412278d0-af21-593f-990c-33d6161029df' THEN 'Germanic Period, 406–687' WHEN 'bokheim:history-lcc:acf0977d-045c-5d24-9a86-f2a5337a9f22' THEN 'Carolingian & Imperial Rule, 687–1291' WHEN 'bokheim:history-lcc:3c17b101-5cb2-5c2a-8632-b8889edea122' THEN 'Old Swiss Confederacy, 1291–1516' WHEN 'bokheim:history-lcc:0b0279b2-146a-5dec-9d29-e0b7c99c6d7d' THEN 'Early Modern Confederacy, 1516–1798' WHEN 'bokheim:history-lcc-full:2a4e3bf1-c522-5d5d-8d2f-c099b1d7ae3e' THEN 'Reformation & Consolidation, 1516–1648' WHEN 'bokheim:history-lcc:ca188862-c9b8-58ce-8404-04485152ff36' THEN 'Revolutionary & Napoleonic Era, 1789–1815' WHEN 'bokheim:history-lcc-full:4d670003-87e0-51f1-b402-3e46d931d595' THEN 'Helvetic Republic, 1798–1803' WHEN 'bokheim:history-lcc:af2b8c64-64a3-539d-8ad4-d1173f4385fe' THEN 'Restoration, 1815–1830' WHEN 'bokheim:history-lcc:f0ed96d0-6391-5dc9-a74e-bc7125f6b155' THEN 'Regeneration, 1830–1848' WHEN 'bokheim:history-lcc-full:dc286a7d-5109-5f79-9e8a-5294ca872cae' THEN 'Sonderbund War, 1847' WHEN 'bokheim:history-lcc:9e6c42c0-7633-5e96-b380-6f943b152fa2' THEN 'Early Federal State, 1848–1900' WHEN 'bokheim:history-lcc-full:0f53ab0d-15b9-54c2-bf64-043414c679b9' THEN 'World Wars Era, 1900–1945' WHEN 'bokheim:history-lcc-full:45f217da-3e49-55b4-8a88-89c2cbc5c326' THEN 'Postwar Switzerland, 1945–2000' WHEN 'bokheim:history-lcc:189714c8-464a-5e5b-9bb2-8dd50b08b43f' THEN 'Contemporary Switzerland, 2000–Present' ELSE preferred_label END WHERE concept_id IN (SELECT concept_id FROM ch_keep);
DROP TABLE ch_edge; DROP TABLE ch_shared; DROP TABLE ch_keep; DROP TABLE ch_tree;
COMMIT;

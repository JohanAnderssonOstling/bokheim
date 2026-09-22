-- Separate Benelux countries from shared Low Countries periods.
PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;
INSERT OR IGNORE INTO concept VALUES('bokheim:benelux:by-period','By period');

CREATE TEMP TABLE bx_old(concept_id TEXT PRIMARY KEY) WITHOUT ROWID;
WITH RECURSIVE t(id) AS (
 SELECT p.concept_id FROM concept_parent p WHERE p.parent_concept_id='unified:f1ffd3a1-ae5d-5720-9659-545b78d8ae0e'
 AND p.concept_id NOT IN ('unified:ecf16473-70d0-54e4-831c-b49dc25bb9ea','unified:f9d54924-adb7-5cc4-8896-f87ed984e468','unified:a9b40fd5-19b3-5adc-8354-44d08510a2f0')
 UNION SELECT p.concept_id FROM t JOIN concept_parent p ON p.parent_concept_id=t.id
) INSERT INTO bx_old SELECT id FROM t;

-- Consolidate narrow medieval ranges into the retained medieval period.
CREATE TEMP TABLE bx_move(source_id TEXT,target_id TEXT,system_id TEXT,selector TEXT,PRIMARY KEY(source_id,system_id,selector)) WITHOUT ROWID;
INSERT INTO bx_move
SELECT s.concept_id,'bokheim:history-lcc-full:0a178a34-7c0c-5f64-8806-e74fb16276e5',s.system_id,s.selector FROM source_selector s WHERE s.concept_id IN
('bokheim:history-lcc-full:d68d600a-b2dd-56a7-8d42-c33a099307ee','bokheim:history-lcc-full:63901a2c-94ea-5949-bba0-47fbb27a9d6c','bokheim:history-lcc-full:69ca37f3-91bd-56ac-95ab-81352d702c75')
UNION ALL SELECT s.concept_id,'bokheim:benelux:by-period',s.system_id,s.selector FROM source_selector s WHERE s.concept_id='bokheim:history-lcc-full:1aef90a1-f51f-586a-a61e-39a8475b28f2'
UNION ALL SELECT s.concept_id,'unified:f1ffd3a1-ae5d-5720-9659-545b78d8ae0e',s.system_id,s.selector FROM source_selector s WHERE s.concept_id='bokheim:history-lcc-full:f060521b-c178-52e2-8026-dc8f82168df1';
WITH numbered AS (SELECT m.*,row_number() OVER(PARTITION BY target_id,system_id ORDER BY selector)-1 rn FROM bx_move m WHERE NOT EXISTS (SELECT 1 FROM source_selector s WHERE s.concept_id=m.target_id AND s.system_id=m.system_id AND s.selector=m.selector))
INSERT OR IGNORE INTO source_selector(concept_id,system_id,selector,ordinal)
SELECT target_id,system_id,selector,COALESCE((SELECT max(s.ordinal)+1 FROM source_selector s WHERE s.concept_id=numbered.target_id AND s.system_id=numbered.system_id),0)+rn FROM numbered;

CREATE TEMP TABLE bx_keep(concept_id TEXT PRIMARY KEY) WITHOUT ROWID;
INSERT INTO bx_keep VALUES
('bokheim:history-lcc-full:e9183f97-5438-53d0-8316-012c92f88a31'),('bokheim:history-lcc-full:9514cd6f-bf78-5e14-98bf-7d30cbf3d07e'),('bokheim:history-lcc-full:0a178a34-7c0c-5f64-8806-e74fb16276e5'),('bokheim:history-lcc-full:38c7ce5c-0c56-5185-b77f-be2557109dfa'),('bokheim:history-lcc-full:f155cb60-087d-5f57-9f5e-b63780b2f2c0');

DELETE FROM concept_parent WHERE parent_concept_id='unified:f1ffd3a1-ae5d-5720-9659-545b78d8ae0e' AND concept_id IN (SELECT concept_id FROM bx_old);
DELETE FROM concept_parent WHERE parent_concept_id IN (SELECT concept_id FROM bx_old);
DELETE FROM concept_parent WHERE concept_id IN (SELECT concept_id FROM bx_old WHERE concept_id NOT IN (SELECT concept_id FROM bx_keep));
DELETE FROM concept WHERE concept_id IN (SELECT concept_id FROM bx_old WHERE concept_id NOT IN (SELECT concept_id FROM bx_keep));

INSERT INTO concept_parent VALUES('bokheim:benelux:by-period','unified:f1ffd3a1-ae5d-5720-9659-545b78d8ae0e',0,10);
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal,display_order) VALUES
('bokheim:history-lcc-full:e9183f97-5438-53d0-8316-012c92f88a31','bokheim:benelux:by-period',0,10),
('bokheim:history-lcc-full:9514cd6f-bf78-5e14-98bf-7d30cbf3d07e','bokheim:benelux:by-period',0,20),
('bokheim:history-lcc-full:0a178a34-7c0c-5f64-8806-e74fb16276e5','bokheim:benelux:by-period',0,30),
('bokheim:history-lcc-full:38c7ce5c-0c56-5185-b77f-be2557109dfa','bokheim:benelux:by-period',0,40),
('bokheim:history-lcc-full:f155cb60-087d-5f57-9f5e-b63780b2f2c0','bokheim:benelux:by-period',0,50);
UPDATE concept SET preferred_label=CASE concept_id
WHEN 'bokheim:history-lcc-full:e9183f97-5438-53d0-8316-012c92f88a31' THEN 'Roman Low Countries'
WHEN 'bokheim:history-lcc-full:9514cd6f-bf78-5e14-98bf-7d30cbf3d07e' THEN 'Frankish Low Countries'
WHEN 'bokheim:history-lcc-full:0a178a34-7c0c-5f64-8806-e74fb16276e5' THEN 'Medieval Low Countries, 843–1384'
WHEN 'bokheim:history-lcc-full:38c7ce5c-0c56-5185-b77f-be2557109dfa' THEN 'Burgundian Netherlands, 1384–1477'
WHEN 'bokheim:history-lcc-full:f155cb60-087d-5f57-9f5e-b63780b2f2c0' THEN 'Habsburg Netherlands, 1477–1555' ELSE preferred_label END
WHERE concept_id IN (SELECT concept_id FROM bx_keep);
DROP TABLE bx_keep; DROP TABLE bx_move; DROP TABLE bx_old;
COMMIT;

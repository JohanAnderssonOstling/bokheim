-- Separate Europe-wide periods from geographic regions and generic topics.
PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;
INSERT OR IGNORE INTO concept VALUES('bokheim:europe:by-period','By period');

CREATE TEMP TABLE eu_move(source_id TEXT,target_id TEXT,system_id TEXT,selector TEXT,PRIMARY KEY(source_id,system_id,selector)) WITHOUT ROWID;
INSERT INTO eu_move
SELECT s.concept_id,'bokheim:history-lcc-full:aba338d1-aa79-5cbd-856b-2e7bc2c22b62',s.system_id,s.selector FROM source_selector s WHERE s.concept_id='bokheim:history-lcc-full:09df06b6-e18b-54f1-bb8e-6b6a6b345354'
UNION ALL SELECT s.concept_id,'bokheim:history-lcc-full:2ee0d9ef-8ed9-5253-b3ea-5eaecd3f2996',s.system_id,s.selector FROM source_selector s WHERE s.concept_id='bokheim:history-lcc-full:3ffa3600-71a0-5663-8e36-bd8c79cf7302'
UNION ALL SELECT s.concept_id,'bokheim:history-lcc-full:49bd468a-f842-570b-9334-f1118e87134e',s.system_id,s.selector FROM source_selector s WHERE s.concept_id IN ('bokheim:history-lcc-full:da316727-3207-5669-bae9-ff8485b7c773','bokheim:history-lcc-full:021bd01f-9970-541e-8dd0-958238bd254c','bokheim:history-lcc-full:c9c79c56-5465-53ed-86c5-279409a683da')
UNION ALL SELECT s.concept_id,(SELECT e.concept_id FROM concept e WHERE e.preferred_label='Europe' AND EXISTS (SELECT 1 FROM concept_parent ep JOIN concept pp ON pp.concept_id=ep.parent_concept_id WHERE ep.concept_id=e.concept_id AND pp.preferred_label='History by Region')),s.system_id,s.selector FROM source_selector s WHERE s.concept_id IN ('bokheim:history-lcc-full:409acc15-d11e-5209-950b-69fda9de8555','bokheim:history-lcc-full:99159bbc-7239-5c41-9ceb-fec4c63ef530');
WITH numbered AS (SELECT m.*,row_number() OVER(PARTITION BY target_id,system_id ORDER BY selector)-1 rn FROM eu_move m WHERE NOT EXISTS (SELECT 1 FROM source_selector s WHERE s.concept_id=m.target_id AND s.system_id=m.system_id AND s.selector=m.selector))
INSERT OR IGNORE INTO source_selector(concept_id,system_id,selector,ordinal)
SELECT target_id,system_id,selector,COALESCE((SELECT max(s.ordinal)+1 FROM source_selector s WHERE s.concept_id=numbered.target_id AND s.system_id=numbered.system_id),0)+rn FROM numbered;

DELETE FROM concept_parent WHERE concept_id IN ('bokheim:history-lcc-full:aba338d1-aa79-5cbd-856b-2e7bc2c22b62','bokheim:history-lcc-full:2ee0d9ef-8ed9-5253-b3ea-5eaecd3f2996','bokheim:history-lcc-full:49bd468a-f842-570b-9334-f1118e87134e','bokheim:history-lcc-full:409acc15-d11e-5209-950b-69fda9de8555','bokheim:history-lcc-full:99159bbc-7239-5c41-9ceb-fec4c63ef530');
DELETE FROM concept_parent WHERE concept_id IN ('bokheim:history-lcc-full:09df06b6-e18b-54f1-bb8e-6b6a6b345354','bokheim:history-lcc-full:3ffa3600-71a0-5663-8e36-bd8c79cf7302','bokheim:history-lcc-full:da316727-3207-5669-bae9-ff8485b7c773','bokheim:history-lcc-full:021bd01f-9970-541e-8dd0-958238bd254c','bokheim:history-lcc-full:c9c79c56-5465-53ed-86c5-279409a683da');
DELETE FROM concept_parent WHERE parent_concept_id IN ('bokheim:history-lcc-full:09df06b6-e18b-54f1-bb8e-6b6a6b345354','bokheim:history-lcc-full:3ffa3600-71a0-5663-8e36-bd8c79cf7302','bokheim:history-lcc-full:da316727-3207-5669-bae9-ff8485b7c773','bokheim:history-lcc-full:021bd01f-9970-541e-8dd0-958238bd254c','bokheim:history-lcc-full:c9c79c56-5465-53ed-86c5-279409a683da');
DELETE FROM concept WHERE concept_id IN ('bokheim:history-lcc-full:09df06b6-e18b-54f1-bb8e-6b6a6b345354','bokheim:history-lcc-full:3ffa3600-71a0-5663-8e36-bd8c79cf7302','bokheim:history-lcc-full:da316727-3207-5669-bae9-ff8485b7c773','bokheim:history-lcc-full:021bd01f-9970-541e-8dd0-958238bd254c','bokheim:history-lcc-full:c9c79c56-5465-53ed-86c5-279409a683da','bokheim:history-lcc-full:409acc15-d11e-5209-950b-69fda9de8555','bokheim:history-lcc-full:99159bbc-7239-5c41-9ceb-fec4c63ef530') AND NOT EXISTS (SELECT 1 FROM concept_parent p WHERE p.concept_id=concept.concept_id);

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal,display_order)
SELECT 'bokheim:europe:by-period',e.concept_id,0,70 FROM concept e WHERE e.preferred_label='Europe' AND EXISTS (SELECT 1 FROM concept_parent ep JOIN concept pp ON pp.concept_id=ep.parent_concept_id WHERE ep.concept_id=e.concept_id AND pp.preferred_label='History by Region');
INSERT INTO concept_parent VALUES
('bokheim:history-lcc-full:aba338d1-aa79-5cbd-856b-2e7bc2c22b62','bokheim:europe:by-period',0,10),
('bokheim:history-lcc-full:2ee0d9ef-8ed9-5253-b3ea-5eaecd3f2996','bokheim:europe:by-period',0,20),
('bokheim:history-lcc-full:49bd468a-f842-570b-9334-f1118e87134e','bokheim:europe:by-period',0,30);
INSERT INTO concept_parent VALUES('bokheim:history-lcc-full:aef4d047-67aa-5ed2-bade-e6b59a1bdd06','bokheim:history-lcc-full:aba338d1-aa79-5cbd-856b-2e7bc2c22b62',0,10);
UPDATE concept SET preferred_label='European Integration' WHERE concept_id='bokheim:history-lcc-full:aef4d047-67aa-5ed2-bade-e6b59a1bdd06';
UPDATE concept SET preferred_label='Postwar Europe, 1945–1989' WHERE concept_id='bokheim:history-lcc-full:aba338d1-aa79-5cbd-856b-2e7bc2c22b62';
UPDATE concept SET preferred_label='Post-Cold War Europe, 1989–2001' WHERE concept_id='bokheim:history-lcc-full:2ee0d9ef-8ed9-5253-b3ea-5eaecd3f2996';
UPDATE concept SET preferred_label='Contemporary Europe, 2001–Present' WHERE concept_id='bokheim:history-lcc-full:49bd468a-f842-570b-9334-f1118e87134e';
DROP TABLE eu_move;
COMMIT;

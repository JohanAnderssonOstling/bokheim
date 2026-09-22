-- Restore Reference and Research as direct Social Sciences subjects with their original BISAC labels.
PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;
INSERT INTO concept VALUES('bokheim:social-sciences:reference','Reference');
INSERT INTO concept_parent VALUES('bokheim:social-sciences:reference','unified:f8f652e6-6244-5dc5-97b7-9dece2de275f',0,90);
UPDATE source_selector SET concept_id='bokheim:social-sciences:reference',ordinal=0 WHERE concept_id='bokheim:sociology:theory-research' AND system_id='bisac' AND selector='SOC023000';
INSERT INTO concept VALUES('bokheim:social-sciences:research','Research');
INSERT INTO concept_parent VALUES('bokheim:social-sciences:research','unified:f8f652e6-6244-5dc5-97b7-9dece2de275f',0,91);
UPDATE source_selector SET concept_id='bokheim:social-sciences:research',ordinal=0 WHERE concept_id='bokheim:sociology:theory-research' AND system_id='bisac' AND selector='SOC024000';
UPDATE concept_parent SET parent_concept_id='unified:f8f652e6-6244-5dc5-97b7-9dece2de275f',display_order=92 WHERE concept_id='unified:1becf5e2-c003-5d8f-bc91-3555cfc1edb8' AND parent_concept_id='bokheim:sociology:theory-research';
UPDATE concept_parent SET parent_concept_id='unified:f8f652e6-6244-5dc5-97b7-9dece2de275f',display_order=93 WHERE concept_id='unified:79a88c35-2392-57fa-b32e-10525ce681d8' AND parent_concept_id='bokheim:sociology:theory-research';
UPDATE concept_parent SET parent_concept_id='unified:f8f652e6-6244-5dc5-97b7-9dece2de275f',display_order=94 WHERE concept_id='unified:abfe73ca-a4af-5f18-8d07-ac55a812faa5' AND parent_concept_id='bokheim:sociology:theory-research';
DELETE FROM concept WHERE concept_id='bokheim:sociology:theory-research';
COMMIT;

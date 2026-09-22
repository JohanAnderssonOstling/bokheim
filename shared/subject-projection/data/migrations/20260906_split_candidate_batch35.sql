PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;

-- 56. Philosophy and social aspects of science.
INSERT INTO concept VALUES ('bokheim:science-philosophy:general','Philosophy & Social Aspects of Science—General'),('bokheim:science-philosophy:special-topics','Philosophy of Science—Special Topics');
INSERT INTO concept_parent VALUES ('bokheim:science-philosophy:general','unified:bbf9b367-9b6f-5d4f-90a4-3a7074b9dd0a',0,100),('bokheim:science-philosophy:special-topics','unified:bbf9b367-9b6f-5d4f-90a4-3a7074b9dd0a',0,200);
UPDATE source_selector SET concept_id='bokheim:science-philosophy:special-topics' WHERE concept_id='unified:bbf9b367-9b6f-5d4f-90a4-3a7074b9dd0a' AND system_id='lcc' AND selector LIKE 'Q175.32%';
UPDATE source_selector SET concept_id='bokheim:science-philosophy:general' WHERE concept_id='unified:bbf9b367-9b6f-5d4f-90a4-3a7074b9dd0a';

COMMIT;

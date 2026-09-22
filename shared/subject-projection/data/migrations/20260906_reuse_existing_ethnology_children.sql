-- Reuse the existing curated anthropology subjects instead of retaining duplicate
-- concepts introduced by the broad Ethnology split.
PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;
DELETE FROM concept_parent WHERE concept_id IN ('unified:1a53d97f-12d1-58fb-a7e3-0657cc840407','unified:3e418b23-c65f-5cec-b6b7-d13b64daa582');
INSERT INTO concept_parent VALUES
 ('unified:1a53d97f-12d1-58fb-a7e3-0657cc840407','unified:0ccbefe9-de72-534e-8464-d95aedd9fd62',0,500),
 ('unified:3e418b23-c65f-5cec-b6b7-d13b64daa582','unified:0ccbefe9-de72-534e-8464-d95aedd9fd62',0,600);
UPDATE source_selector SET concept_id='unified:1a53d97f-12d1-58fb-a7e3-0657cc840407',ordinal=100
 WHERE concept_id='bokheim:ethnology:traits';
UPDATE source_selector SET concept_id='unified:3e418b23-c65f-5cec-b6b7-d13b64daa582',ordinal=100
 WHERE concept_id='bokheim:ethnology:groups';
DELETE FROM concept WHERE concept_id IN ('bokheim:ethnology:traits','bokheim:ethnology:groups');
COMMIT;

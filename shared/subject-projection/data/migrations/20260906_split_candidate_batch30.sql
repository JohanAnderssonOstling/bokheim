PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;

-- 74. Courts and judicial institutions.
INSERT INTO concept VALUES ('bokheim:courts:organization-procedure','Courts, Organization & Procedure'),('bokheim:courts:jury','Juries & Jury Law'),('bokheim:courts:judicial-power','Judicial Power');
INSERT INTO concept_parent VALUES ('bokheim:courts:organization-procedure','bokheim:law:procedure:courts-judiciary',0,100),('bokheim:courts:jury','bokheim:law:procedure:courts-judiciary',0,200),('bokheim:courts:judicial-power','bokheim:law:procedure:courts-judiciary',0,300);
UPDATE source_selector SET concept_id='bokheim:courts:jury',ordinal=0 WHERE concept_id='bokheim:law:procedure:courts-judiciary' AND system_id='bisac' AND selector='LAW053000';
UPDATE source_selector SET concept_id='bokheim:courts:judicial-power',ordinal=0 WHERE concept_id='bokheim:law:procedure:courts-judiciary' AND system_id='bisac' AND selector='LAW111000';
UPDATE source_selector SET concept_id='bokheim:courts:organization-procedure' WHERE concept_id='bokheim:law:procedure:courts-judiciary';

COMMIT;

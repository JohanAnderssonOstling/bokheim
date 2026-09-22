PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;

-- 75. Legal history and Roman law.
INSERT INTO concept VALUES ('bokheim:legal-history:general','Legal History—General'),('bokheim:legal-history:eastern-canon','History of Eastern Catholic Canon Law'),('bokheim:legal-history:roman','Roman Law & Legal History');
INSERT INTO concept_parent VALUES ('bokheim:legal-history:general','bokheim:subject:law:systems:history',0,100),('bokheim:legal-history:eastern-canon','bokheim:subject:law:systems:history',0,200),('bokheim:legal-history:roman','bokheim:subject:law:systems:history',0,300);
UPDATE source_selector SET concept_id='bokheim:legal-history:eastern-canon' WHERE concept_id='bokheim:subject:law:systems:history' AND system_id='lcc' AND selector LIKE 'KBT%';
UPDATE source_selector SET concept_id='bokheim:legal-history:roman' WHERE concept_id='bokheim:subject:law:systems:history' AND system_id='lcc' AND selector LIKE 'KJA%';
UPDATE source_selector SET concept_id='bokheim:legal-history:general' WHERE concept_id='bokheim:subject:law:systems:history';

COMMIT;

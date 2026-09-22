PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;

-- 91. Separate the exact antitrust heading from the cross-jurisdiction IP schedule.
INSERT INTO concept VALUES ('bokheim:business-law:intellectual-property','Intellectual Property & Related Competition Law'),('bokheim:business-law:antitrust','Antitrust Law');
INSERT INTO concept_parent VALUES ('bokheim:business-law:intellectual-property','bokheim:subject:law:business:ip-competition',0,100),('bokheim:business-law:antitrust','bokheim:subject:law:business:ip-competition',0,200);
UPDATE source_selector SET concept_id='bokheim:business-law:antitrust',ordinal=0 WHERE concept_id='bokheim:subject:law:business:ip-competition' AND system_id='bisac' AND selector='LAW005000';
UPDATE source_selector SET concept_id='bokheim:business-law:intellectual-property' WHERE concept_id='bokheim:subject:law:business:ip-competition';

COMMIT;

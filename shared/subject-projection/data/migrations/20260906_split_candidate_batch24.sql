PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;

-- 92. Replace hundreds of generated cutter variants with four substantive leaves.
INSERT INTO concept VALUES ('bokheim:abuse:general','Abuse of Persons—General'),('bokheim:abuse:domestic','Family & Domestic Violence'),('bokheim:abuse:child','Child Abuse'),('bokheim:abuse:other','Other Forms of Abuse');
INSERT INTO concept_parent VALUES ('bokheim:abuse:general','bokheim:curation:2c7f9d2c-b978-5483-90de-9980ad734a6e',0,100),('bokheim:abuse:domestic','bokheim:curation:2c7f9d2c-b978-5483-90de-9980ad734a6e',0,200),('bokheim:abuse:child','bokheim:curation:2c7f9d2c-b978-5483-90de-9980ad734a6e',0,300),('bokheim:abuse:other','bokheim:curation:2c7f9d2c-b978-5483-90de-9980ad734a6e',0,400);
DELETE FROM source_selector WHERE concept_id='bokheim:curation:2c7f9d2c-b978-5483-90de-9980ad734a6e';
INSERT INTO source_selector VALUES ('bokheim:abuse:general','lcc','HV6625..HV6625.99@4',0),('bokheim:abuse:domestic','lcc','HV6626..HV6626.49@4',0),('bokheim:abuse:child','lcc','HV6626.5..HV6626.54@4',0),('bokheim:abuse:other','lcc','HV6626.55..HV6626.7@4',0);

COMMIT;

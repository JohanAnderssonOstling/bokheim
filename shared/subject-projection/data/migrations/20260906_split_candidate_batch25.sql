PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;

-- 95. Inscriptions and epigraphy.
INSERT INTO concept VALUES ('bokheim:epigraphy:general','Epigraphy—General, Theory & Method'),('bokheim:epigraphy:ancient','Ancient Inscriptions'),('bokheim:epigraphy:early-christian','Early Christian Inscriptions'),('bokheim:epigraphy:language','Inscriptions by Language'),('bokheim:epigraphy:regional','Inscriptions by Country or Region');
INSERT INTO concept_parent VALUES ('bokheim:epigraphy:general','unified:53d3e525-c60d-57c5-8e8c-9a747a09efa9',0,100),('bokheim:epigraphy:ancient','unified:53d3e525-c60d-57c5-8e8c-9a747a09efa9',0,200),('bokheim:epigraphy:early-christian','unified:53d3e525-c60d-57c5-8e8c-9a747a09efa9',0,300),('bokheim:epigraphy:language','unified:53d3e525-c60d-57c5-8e8c-9a747a09efa9',0,400),('bokheim:epigraphy:regional','unified:53d3e525-c60d-57c5-8e8c-9a747a09efa9',0,500);
DELETE FROM source_selector WHERE concept_id='unified:53d3e525-c60d-57c5-8e8c-9a747a09efa9';
INSERT INTO source_selector VALUES ('bokheim:epigraphy:general','lcc','CN1..CN119.9@3',0),('bokheim:epigraphy:ancient','lcc','CN120..CN749.9@3',0),('bokheim:epigraphy:early-christian','lcc','CN750..CN760@3',0),('bokheim:epigraphy:language','lcc','CN760.1..CN869.9@3',0),('bokheim:epigraphy:regional','lcc','CN870..CN1355@3',0);

COMMIT;

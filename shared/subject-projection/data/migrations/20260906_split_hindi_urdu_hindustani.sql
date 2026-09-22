-- Separate languages from literatures using the five official LCC spans.
PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;
INSERT INTO concept(concept_id,preferred_label) VALUES
 ('bokheim:hindi-urdu:hindi-language','Hindi Language'),
 ('bokheim:hindi-urdu:urdu-language','Urdu Language'),
 ('bokheim:hindi-urdu:hindustani-language','Hindustani Language'),
 ('bokheim:hindi-urdu:hindi-hindustani-literature','Hindi & Hindustani Literature'),
 ('bokheim:hindi-urdu:urdu-literature','Urdu Literature');
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal,display_order) VALUES
 ('bokheim:hindi-urdu:hindi-language','unified:c02e1693-cbe6-5f2f-a1b4-a12dd90a9a47',0,100),
 ('bokheim:hindi-urdu:urdu-language','unified:c02e1693-cbe6-5f2f-a1b4-a12dd90a9a47',0,200),
 ('bokheim:hindi-urdu:hindustani-language','unified:c02e1693-cbe6-5f2f-a1b4-a12dd90a9a47',0,300),
 ('bokheim:hindi-urdu:hindi-hindustani-literature','unified:c02e1693-cbe6-5f2f-a1b4-a12dd90a9a47',0,400),
 ('bokheim:hindi-urdu:urdu-literature','unified:c02e1693-cbe6-5f2f-a1b4-a12dd90a9a47',0,500);
DELETE FROM source_selector WHERE concept_id='unified:c02e1693-cbe6-5f2f-a1b4-a12dd90a9a47' AND selector='PK1931..PK2212@5';
UPDATE source_selector SET concept_id='bokheim:hindi-urdu:hindi-language',ordinal=0 WHERE concept_id='unified:c02e1693-cbe6-5f2f-a1b4-a12dd90a9a47' AND selector='PK1931..PK1970@6';
UPDATE source_selector SET concept_id='bokheim:hindi-urdu:urdu-language',ordinal=0 WHERE concept_id='unified:c02e1693-cbe6-5f2f-a1b4-a12dd90a9a47' AND selector='PK1971..PK1979.5@6';
UPDATE source_selector SET concept_id='bokheim:hindi-urdu:hindustani-language',ordinal=0 WHERE concept_id='unified:c02e1693-cbe6-5f2f-a1b4-a12dd90a9a47' AND selector='PK1981..PK2000@6';
UPDATE source_selector SET concept_id='bokheim:hindi-urdu:hindi-hindustani-literature',ordinal=0 WHERE concept_id='unified:c02e1693-cbe6-5f2f-a1b4-a12dd90a9a47' AND selector='PK2030..PK2142@6';
UPDATE source_selector SET concept_id='bokheim:hindi-urdu:urdu-literature',ordinal=0 WHERE concept_id='unified:c02e1693-cbe6-5f2f-a1b4-a12dd90a9a47' AND selector='PK2151..PK2212@6';
COMMIT;

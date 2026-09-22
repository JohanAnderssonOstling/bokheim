-- Split Accounting using the exact BISAC subdivisions already attached.
PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;
INSERT INTO concept(concept_id,preferred_label) VALUES
 ('bokheim:accounting:general','General Accounting'),
 ('bokheim:accounting:financial','Financial Accounting'),
 ('bokheim:accounting:governmental','Governmental Accounting'),
 ('bokheim:accounting:managerial','Managerial Accounting'),
 ('bokheim:accounting:standards','Accounting Standards');
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal,display_order) VALUES
 ('bokheim:accounting:general','unified:1055eec5-b7c3-519d-85d3-420d9ec68986',0,100),
 ('bokheim:accounting:financial','unified:1055eec5-b7c3-519d-85d3-420d9ec68986',0,200),
 ('bokheim:accounting:governmental','unified:1055eec5-b7c3-519d-85d3-420d9ec68986',0,300),
 ('bokheim:accounting:managerial','unified:1055eec5-b7c3-519d-85d3-420d9ec68986',0,400),
 ('bokheim:accounting:standards','unified:1055eec5-b7c3-519d-85d3-420d9ec68986',0,500);
UPDATE source_selector SET concept_id='bokheim:accounting:general',ordinal=0 WHERE concept_id='unified:1055eec5-b7c3-519d-85d3-420d9ec68986' AND system_id='bisac' AND selector='BUS001000';
UPDATE source_selector SET concept_id='bokheim:accounting:general',ordinal=0 WHERE concept_id='unified:1055eec5-b7c3-519d-85d3-420d9ec68986' AND system_id='lcc' AND selector='HF5601..HF5689@1';
UPDATE source_selector SET concept_id='bokheim:accounting:general',ordinal=1 WHERE concept_id='unified:1055eec5-b7c3-519d-85d3-420d9ec68986' AND system_id='lcc' AND selector='HF5601-5688';
UPDATE source_selector SET concept_id='bokheim:accounting:financial',ordinal=0 WHERE concept_id='unified:1055eec5-b7c3-519d-85d3-420d9ec68986' AND selector='BUS001010';
UPDATE source_selector SET concept_id='bokheim:accounting:governmental',ordinal=0 WHERE concept_id='unified:1055eec5-b7c3-519d-85d3-420d9ec68986' AND selector='BUS001020';
UPDATE source_selector SET concept_id='bokheim:accounting:managerial',ordinal=0 WHERE concept_id='unified:1055eec5-b7c3-519d-85d3-420d9ec68986' AND selector='BUS001040';
UPDATE source_selector SET concept_id='bokheim:accounting:standards',ordinal=0 WHERE concept_id='unified:1055eec5-b7c3-519d-85d3-420d9ec68986' AND selector='BUS001050';
COMMIT;

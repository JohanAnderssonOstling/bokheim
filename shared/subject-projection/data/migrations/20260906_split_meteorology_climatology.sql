-- Replace overlapping meteorology selectors with five disjoint broad leaves.
PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;
INSERT INTO concept(concept_id,preferred_label) VALUES
 ('bokheim:meteorology:general-observation','General Meteorology & Observation'),
 ('bokheim:meteorology:atmosphere','Atmospheric Physics & Dynamics'),
 ('bokheim:meteorology:phenomena','Atmospheric Electrical & Optical Phenomena'),
 ('bokheim:meteorology:climatology-weather','Climatology, Weather & Forecasting'),
 ('bokheim:meteorology:engineering','Engineering Meteorology');
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal,display_order) VALUES
 ('bokheim:meteorology:general-observation','unified:8c74843a-65e7-5225-9f99-01ecbeba18ad',0,100),
 ('bokheim:meteorology:atmosphere','unified:8c74843a-65e7-5225-9f99-01ecbeba18ad',0,200),
 ('bokheim:meteorology:phenomena','unified:8c74843a-65e7-5225-9f99-01ecbeba18ad',0,300),
 ('bokheim:meteorology:climatology-weather','unified:8c74843a-65e7-5225-9f99-01ecbeba18ad',0,400),
 ('bokheim:meteorology:engineering','unified:8c74843a-65e7-5225-9f99-01ecbeba18ad',0,500);
DELETE FROM source_selector WHERE concept_id='unified:8c74843a-65e7-5225-9f99-01ecbeba18ad';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) VALUES
 ('bokheim:meteorology:general-observation','bisac','SCI042000',0),
 ('bokheim:meteorology:general-observation','lcc','QC851..QC879.9@3',0),
 ('bokheim:meteorology:atmosphere','lcc','QC880..QC959.9@3',0),
 ('bokheim:meteorology:phenomena','lcc','QC960..QC979.9@3',0),
 ('bokheim:meteorology:climatology-weather','lcc','QC980..QC999@3',0),
 ('bokheim:meteorology:engineering','lcc','TA197..TA198@2',0);
COMMIT;

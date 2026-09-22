PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;

-- 62. Economic methodology.
INSERT INTO concept VALUES ('bokheim:economic-methodology:general','Economic Methodology—General'),('bokheim:economic-methodology:information','Information Theory & Economic Cybernetics'),('bokheim:economic-methodology:quantitative','Mathematical Economics & Quantitative Methods'),('bokheim:economic-methodology:econometrics','Econometrics');
INSERT INTO concept_parent VALUES ('bokheim:economic-methodology:general','unified:4ad4c662-a6ef-5130-afb4-8ca85f3adc51',0,100),('bokheim:economic-methodology:information','unified:4ad4c662-a6ef-5130-afb4-8ca85f3adc51',0,200),('bokheim:economic-methodology:quantitative','unified:4ad4c662-a6ef-5130-afb4-8ca85f3adc51',0,300),('bokheim:economic-methodology:econometrics','unified:4ad4c662-a6ef-5130-afb4-8ca85f3adc51',0,400);
DELETE FROM source_selector WHERE concept_id='unified:4ad4c662-a6ef-5130-afb4-8ca85f3adc51';
INSERT INTO source_selector VALUES ('bokheim:economic-methodology:general','lcc','HB131..HB132.9@3',0),('bokheim:economic-methodology:information','lcc','HB133..HB134.9@3',0),('bokheim:economic-methodology:quantitative','lcc','HB135..HB138.9@4',0),('bokheim:economic-methodology:econometrics','lcc','HB139..HB147@4',0);

COMMIT;

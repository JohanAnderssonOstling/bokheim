-- Split demography into its principal official HB divisions.
PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;
INSERT INTO concept(concept_id,preferred_label) VALUES
 ('bokheim:demography:general','General Demography'),
 ('bokheim:demography:fertility','Births & Fertility'),
 ('bokheim:demography:marriage','Marriage & Nuptiality'),
 ('bokheim:demography:mortality','Deaths & Mortality'),
 ('bokheim:demography:age','Age & Age Distribution'),
 ('bokheim:demography:sex','Sex & Population'),
 ('bokheim:demography:migration','Population Geography & Migration'),
 ('bokheim:demography:occupations','Professions & Occupations'),
 ('bokheim:demography:ethnic-national','Nationalities & Ethnic Groups'),
 ('bokheim:demography:regions','Demography by Region or Country');
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal,display_order) VALUES
 ('bokheim:demography:general','unified:b1dbdf6f-d14a-5ac2-903f-8539b1e38d6a',0,100),
 ('bokheim:demography:fertility','unified:b1dbdf6f-d14a-5ac2-903f-8539b1e38d6a',0,200),
 ('bokheim:demography:marriage','unified:b1dbdf6f-d14a-5ac2-903f-8539b1e38d6a',0,300),
 ('bokheim:demography:mortality','unified:b1dbdf6f-d14a-5ac2-903f-8539b1e38d6a',0,400),
 ('bokheim:demography:age','unified:b1dbdf6f-d14a-5ac2-903f-8539b1e38d6a',0,500),
 ('bokheim:demography:sex','unified:b1dbdf6f-d14a-5ac2-903f-8539b1e38d6a',0,600),
 ('bokheim:demography:migration','unified:b1dbdf6f-d14a-5ac2-903f-8539b1e38d6a',0,700),
 ('bokheim:demography:occupations','unified:b1dbdf6f-d14a-5ac2-903f-8539b1e38d6a',0,800),
 ('bokheim:demography:ethnic-national','unified:b1dbdf6f-d14a-5ac2-903f-8539b1e38d6a',0,900),
 ('bokheim:demography:regions','unified:b1dbdf6f-d14a-5ac2-903f-8539b1e38d6a',0,1000);
DELETE FROM source_selector WHERE concept_id='unified:b1dbdf6f-d14a-5ac2-903f-8539b1e38d6a';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) VALUES
 ('bokheim:demography:general','lcc','HB848..HB900.9@4',0),
 ('bokheim:demography:general','lcc','HB848-3697',1),
 ('bokheim:demography:fertility','lcc','HB901..HB1110.9@4',0),
 ('bokheim:demography:marriage','lcc','HB1111..HB1320.9@4',0),
 ('bokheim:demography:mortality','lcc','HB1321..HB1530.9@4',0),
 ('bokheim:demography:age','lcc','HB1531..HB1740.9@4',0),
 ('bokheim:demography:sex','lcc','HB1741..HB1950.9@4',0),
 ('bokheim:demography:migration','lcc','HB1951..HB2580.9@4',0),
 ('bokheim:demography:occupations','lcc','HB2581..HB2787@4',0),
 ('bokheim:demography:ethnic-national','lcc','HB2787.1..HB3500.9@4',0),
 ('bokheim:demography:regions','lcc','HB3501..HB3697@4',0);
COMMIT;

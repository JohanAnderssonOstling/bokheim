PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;

-- 84. Soil science.
INSERT INTO concept VALUES ('bokheim:soil-science:general','Soil Science—General, History & Research'),('bokheim:soil-science:survey','Soil Surveys & Classification'),('bokheim:soil-science:physics','Soil Physics'),('bokheim:soil-science:chemistry','Soil Chemistry & Biochemistry'),('bokheim:soil-science:fertility','Soil Fertility, Nutrition & Crop Relationships'),('bokheim:soil-science:regional','Soils by Region or Locality');
INSERT INTO concept_parent VALUES ('bokheim:soil-science:general','unified:d1842d7f-919b-533b-b2e1-daf257ca89ea',0,100),('bokheim:soil-science:survey','unified:d1842d7f-919b-533b-b2e1-daf257ca89ea',0,200),('bokheim:soil-science:physics','unified:d1842d7f-919b-533b-b2e1-daf257ca89ea',0,300),('bokheim:soil-science:chemistry','unified:d1842d7f-919b-533b-b2e1-daf257ca89ea',0,400),('bokheim:soil-science:fertility','unified:d1842d7f-919b-533b-b2e1-daf257ca89ea',0,500),('bokheim:soil-science:regional','unified:d1842d7f-919b-533b-b2e1-daf257ca89ea',0,600);
DELETE FROM source_selector WHERE concept_id='unified:d1842d7f-919b-533b-b2e1-daf257ca89ea';
INSERT INTO source_selector VALUES ('bokheim:soil-science:general','bisac','TEC003060',0),('bokheim:soil-science:general','lcc','S590..S592.13@4',0),('bokheim:soil-science:survey','lcc','S592.14..S592.29@4',0),('bokheim:soil-science:physics','lcc','S592.3..S592.49@4',0),('bokheim:soil-science:chemistry','lcc','S592.5..S592.99@4',0),('bokheim:soil-science:fertility','lcc','S593..S598.99@4',0),('bokheim:soil-science:regional','lcc','S599..S599.9@4',0);

-- 85. Birds and beneficial insects.
INSERT INTO concept VALUES ('bokheim:animal-culture:cage-birds','Cage Birds'),('bokheim:animal-culture:poultry','Poultry'),('bokheim:animal-culture:game-birds','Game Birds'),('bokheim:animal-culture:ornamental-birds','Ornamental Birds'),('bokheim:animal-culture:bees','Beekeeping'),('bokheim:animal-culture:silk','Sericulture & Silk Culture');
INSERT INTO concept_parent VALUES ('bokheim:animal-culture:cage-birds','bokheim:animal-culture:birds-insects',0,100),('bokheim:animal-culture:poultry','bokheim:animal-culture:birds-insects',0,200),('bokheim:animal-culture:game-birds','bokheim:animal-culture:birds-insects',0,300),('bokheim:animal-culture:ornamental-birds','bokheim:animal-culture:birds-insects',0,400),('bokheim:animal-culture:bees','bokheim:animal-culture:birds-insects',0,500),('bokheim:animal-culture:silk','bokheim:animal-culture:birds-insects',0,600);
DELETE FROM source_selector WHERE concept_id='bokheim:animal-culture:birds-insects';
INSERT INTO source_selector VALUES ('bokheim:animal-culture:cage-birds','lcc','SF461..SF473@3',0),('bokheim:animal-culture:poultry','lcc','SF481..SF507@3',0),('bokheim:animal-culture:game-birds','lcc','SF508..SF510.6@3',0),('bokheim:animal-culture:ornamental-birds','lcc','SF512..SF513@3',0),('bokheim:animal-culture:bees','lcc','SF521..SF539.8@3',0),('bokheim:animal-culture:silk','lcc','SF541..SF560@3',0);

-- 86. Catholic governance and clergy administration.
INSERT INTO concept VALUES ('bokheim:catholic:church-government','Catholic Church Government & Organization'),('bokheim:catholic:clergy-rules','Catholic Clergy Rules & Ordinances');
INSERT INTO concept_parent VALUES ('bokheim:catholic:church-government','bokheim:catholic:governance-clergy-administration',0,100),('bokheim:catholic:clergy-rules','bokheim:catholic:governance-clergy-administration',0,200);
DELETE FROM source_selector WHERE concept_id='bokheim:catholic:governance-clergy-administration';
INSERT INTO source_selector VALUES ('bokheim:catholic:church-government','lcc','BX1800..BX1920@4',0),('bokheim:catholic:clergy-rules','lcc','BX1930..BX1932@4',0);

COMMIT;

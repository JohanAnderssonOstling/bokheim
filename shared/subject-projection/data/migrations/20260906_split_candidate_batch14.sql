PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;

-- 73. Interactive and multimedia computing.
INSERT INTO concept VALUES ('bokheim:interactive-media:multimedia','Multimedia Systems'),('bokheim:interactive-media:hypertext','Hypertext & Hypermedia'),('bokheim:interactive-media:entertainment','Entertainment Computing'),('bokheim:interactive-media:systems','Interactive Computer Systems');
INSERT INTO concept_parent VALUES ('bokheim:interactive-media:multimedia','unified:223d0593-1831-5635-a876-23699938950c',0,100),('bokheim:interactive-media:hypertext','unified:223d0593-1831-5635-a876-23699938950c',0,200),('bokheim:interactive-media:entertainment','unified:223d0593-1831-5635-a876-23699938950c',0,300),('bokheim:interactive-media:systems','unified:223d0593-1831-5635-a876-23699938950c',0,400);
DELETE FROM source_selector WHERE concept_id='unified:223d0593-1831-5635-a876-23699938950c';
INSERT INTO source_selector VALUES ('bokheim:interactive-media:multimedia','bisac','COM034000',0),('bokheim:interactive-media:multimedia','lcc','QA76.575',0),('bokheim:interactive-media:hypertext','lcc','QA76.76.H94',0),('bokheim:interactive-media:hypertext','lcc','QA76.76.I59',1),('bokheim:interactive-media:entertainment','lcc','QA76.9.E57',0),('bokheim:interactive-media:systems','lcc','QA76.9.I58',0);

-- 78. Special aspects of higher education.
INSERT INTO concept VALUES ('bokheim:higher-ed-aspects:teaching','College Teaching'),('bokheim:higher-ed-aspects:regional','Higher Education by Region or Country'),('bokheim:higher-ed-aspects:staffing','Academic Staffing, Workload & Assistants'),('bokheim:higher-ed-aspects:tenure','Academic Tenure, Dismissal & Leave');
INSERT INTO concept_parent VALUES ('bokheim:higher-ed-aspects:teaching','bokheim:lcc-detail:8a0e73a4-b25e-5203-ac46-693a7d663c27',0,100),('bokheim:higher-ed-aspects:regional','bokheim:lcc-detail:8a0e73a4-b25e-5203-ac46-693a7d663c27',0,200),('bokheim:higher-ed-aspects:staffing','bokheim:lcc-detail:8a0e73a4-b25e-5203-ac46-693a7d663c27',0,300),('bokheim:higher-ed-aspects:tenure','bokheim:lcc-detail:8a0e73a4-b25e-5203-ac46-693a7d663c27',0,400);
DELETE FROM source_selector WHERE concept_id='bokheim:lcc-detail:8a0e73a4-b25e-5203-ac46-693a7d663c27';
INSERT INTO source_selector VALUES ('bokheim:higher-ed-aspects:teaching','lcc','LB2331..LB2333.84@4',0),('bokheim:higher-ed-aspects:regional','lcc','LB2334',0),('bokheim:higher-ed-aspects:staffing','lcc','LB2335.3',0),('bokheim:higher-ed-aspects:staffing','lcc','LB2335.35',1),('bokheim:higher-ed-aspects:staffing','lcc','LB2335.4',2),('bokheim:higher-ed-aspects:staffing','lcc','LB2335.5',3),('bokheim:higher-ed-aspects:tenure','lcc','LB2335.7',0),('bokheim:higher-ed-aspects:tenure','lcc','LB2335.8',1);

-- 79. Hebrew language and literature.
INSERT INTO concept VALUES ('bokheim:hebrew:philology','Hebrew Philology'),('bokheim:hebrew:language','Biblical & Modern Hebrew Language'),('bokheim:hebrew:medieval','Medieval Hebrew'),('bokheim:hebrew:literature','Hebrew Literature'),('bokheim:hebrew:jewish-languages','Other Languages Used by Jews');
INSERT INTO concept_parent VALUES ('bokheim:hebrew:philology','unified:6188f929-779b-51ef-b624-452f41055380',0,100),('bokheim:hebrew:language','unified:6188f929-779b-51ef-b624-452f41055380',0,200),('bokheim:hebrew:medieval','unified:6188f929-779b-51ef-b624-452f41055380',0,300),('bokheim:hebrew:literature','unified:6188f929-779b-51ef-b624-452f41055380',0,400),('bokheim:hebrew:jewish-languages','unified:6188f929-779b-51ef-b624-452f41055380',0,500);
DELETE FROM source_selector WHERE concept_id='unified:6188f929-779b-51ef-b624-452f41055380';
INSERT INTO source_selector VALUES ('bokheim:hebrew:philology','lcc','PJ4501..PJ4541@4',0),('bokheim:hebrew:language','bisac','FOR011000',0),('bokheim:hebrew:language','lcc','PJ4543..PJ4949.9@4',0),('bokheim:hebrew:medieval','lcc','PJ4950..PJ4999.9@4',0),('bokheim:hebrew:literature','lcc','PJ5000..PJ5060@4',0),('bokheim:hebrew:jewish-languages','lcc','PJ5061..PJ5192@4',0);

-- 81. Family forms and kinship.
INSERT INTO concept VALUES ('bokheim:family-forms:general','Family Forms & Kinship—General'),('bokheim:family-forms:adoption','Adoption & Fostering'),('bokheim:family-forms:alternative','Alternative Families'),('bokheim:family-forms:extended','Extended Family'),('bokheim:family-forms:siblings','Siblings'),('bokheim:family-forms:military','Military Families');
INSERT INTO concept_parent VALUES ('bokheim:family-forms:general','bokheim:family:forms-kinship',0,100),('bokheim:family-forms:adoption','bokheim:family:forms-kinship',0,200),('bokheim:family-forms:alternative','bokheim:family:forms-kinship',0,300),('bokheim:family-forms:extended','bokheim:family:forms-kinship',0,400),('bokheim:family-forms:siblings','bokheim:family:forms-kinship',0,500),('bokheim:family-forms:military','bokheim:family:forms-kinship',0,600);
DELETE FROM source_selector WHERE concept_id='bokheim:family:forms-kinship';
INSERT INTO source_selector VALUES ('bokheim:family-forms:general','lcc','HQ731..HQ743@5',0),('bokheim:family-forms:adoption','bisac','FAM004000',0),('bokheim:family-forms:alternative','bisac','FAM006000',0),('bokheim:family-forms:extended','bisac','FAM053000',0),('bokheim:family-forms:siblings','bisac','FAM041000',0),('bokheim:family-forms:military','bisac','FAM055000',0);

-- 82. Immunologic diseases.
INSERT INTO concept VALUES ('bokheim:immunologic-diseases:general','Immunologic Diseases—General'),('bokheim:immunologic-diseases:autoimmune','Autoimmune Diseases'),('bokheim:immunologic-diseases:immunodeficiency','Immunodeficiency Diseases');
INSERT INTO concept_parent VALUES ('bokheim:immunologic-diseases:general','bokheim:lcc-detail:5b815204-1aa6-5b19-8b81-290dc80d0313',0,100),('bokheim:immunologic-diseases:autoimmune','bokheim:lcc-detail:5b815204-1aa6-5b19-8b81-290dc80d0313',0,200),('bokheim:immunologic-diseases:immunodeficiency','bokheim:lcc-detail:5b815204-1aa6-5b19-8b81-290dc80d0313',0,300);
DELETE FROM source_selector WHERE concept_id='bokheim:lcc-detail:5b815204-1aa6-5b19-8b81-290dc80d0313';
INSERT INTO source_selector VALUES ('bokheim:immunologic-diseases:general','lcc','RC581..RC599.9@4',0),('bokheim:immunologic-diseases:autoimmune','lcc','RC600..RC605.9@4',0),('bokheim:immunologic-diseases:immunodeficiency','lcc','RC606..RC607.99@4',0);

-- 83. Tariffs and trade policy.
INSERT INTO concept VALUES ('bokheim:tariff:general','Tariffs, Free Trade & Protectionism—General'),('bokheim:tariff:drawbacks','Tariff Drawbacks'),('bokheim:tariff:preferences','Tariff Preferences & Reciprocity'),('bokheim:tariff:regional','Tariffs by Country or Region'),('bokheim:tariff:export','Export Duties');
INSERT INTO concept_parent VALUES ('bokheim:tariff:general','unified:26a15861-0218-587b-ad20-2f9ea540fc0b',0,100),('bokheim:tariff:drawbacks','unified:26a15861-0218-587b-ad20-2f9ea540fc0b',0,200),('bokheim:tariff:preferences','unified:26a15861-0218-587b-ad20-2f9ea540fc0b',0,300),('bokheim:tariff:regional','unified:26a15861-0218-587b-ad20-2f9ea540fc0b',0,400),('bokheim:tariff:export','unified:26a15861-0218-587b-ad20-2f9ea540fc0b',0,500);
DELETE FROM source_selector WHERE concept_id='unified:26a15861-0218-587b-ad20-2f9ea540fc0b';
INSERT INTO source_selector VALUES ('bokheim:tariff:general','lcc','HF1701..HF1714.9@4',0),('bokheim:tariff:drawbacks','lcc','HF1715..HF1720.9@4',0),('bokheim:tariff:preferences','lcc','HF1721..HF1744.9@4',0),('bokheim:tariff:regional','lcc','HF1745..HF2659.9@4',0),('bokheim:tariff:export','lcc','HF2660..HF2701@4',0);

COMMIT;

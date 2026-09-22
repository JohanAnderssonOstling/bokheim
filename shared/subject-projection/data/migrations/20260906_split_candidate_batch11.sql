PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;

-- 32. Orthodox Christianity: disjoint church/region leaves.
INSERT INTO concept VALUES
 ('bokheim:orthodox:general-east','Orthodox Christianity—General & Eastern Patriarchates'),
 ('bokheim:orthodox:cyprus','Church of Cyprus'),
 ('bokheim:orthodox:russian','Russian Orthodox Church'),
 ('bokheim:orthodox:greek','Greek Orthodox Church'),
 ('bokheim:orthodox:central-europe','Orthodox Churches in Central Europe'),
 ('bokheim:orthodox:balkan-caucasus','Orthodox Churches of the Balkans & Caucasus'),
 ('bokheim:orthodox:serbian-macedonian','Serbian & Macedonian Orthodox Churches'),
 ('bokheim:orthodox:diaspora','Orthodox Diaspora Churches');
INSERT INTO concept_parent VALUES
 ('bokheim:orthodox:general-east','unified:7256864b-1a86-5b68-a4fc-62e41d604684',0,100),
 ('bokheim:orthodox:cyprus','unified:7256864b-1a86-5b68-a4fc-62e41d604684',0,200),
 ('bokheim:orthodox:russian','unified:7256864b-1a86-5b68-a4fc-62e41d604684',0,300),
 ('bokheim:orthodox:greek','unified:7256864b-1a86-5b68-a4fc-62e41d604684',0,400),
 ('bokheim:orthodox:central-europe','unified:7256864b-1a86-5b68-a4fc-62e41d604684',0,500),
 ('bokheim:orthodox:balkan-caucasus','unified:7256864b-1a86-5b68-a4fc-62e41d604684',0,600),
 ('bokheim:orthodox:serbian-macedonian','unified:7256864b-1a86-5b68-a4fc-62e41d604684',0,700),
 ('bokheim:orthodox:diaspora','unified:7256864b-1a86-5b68-a4fc-62e41d604684',0,800);
DELETE FROM source_selector WHERE concept_id='unified:7256864b-1a86-5b68-a4fc-62e41d604684';
INSERT INTO source_selector VALUES
 ('bokheim:orthodox:general-east','bisac','REL049000',0),('bokheim:orthodox:general-east','lcc','BX200..BX449.9@3',0),
 ('bokheim:orthodox:cyprus','lcc','BX450..BX459.9@4',0),
 ('bokheim:orthodox:russian','lcc','BX460..BX609.9@4',0),
 ('bokheim:orthodox:greek','lcc','BX610..BX629.9@4',0),
 ('bokheim:orthodox:central-europe','lcc','BX630..BX649.9@4',0),
 ('bokheim:orthodox:balkan-caucasus','lcc','BX650..BX709.9@4',0),
 ('bokheim:orthodox:serbian-macedonian','lcc','BX710..BX729.89@4',0),
 ('bokheim:orthodox:diaspora','lcc','BX729.9..BX796@4',0);

-- 33. Human ecology and anthropogeography.
INSERT INTO concept VALUES
 ('bokheim:human-ecology:general','Human Ecology & Anthropogeography—General'),
 ('bokheim:human-ecology:settlements','Human Settlements & Population Geography'),
 ('bokheim:human-ecology:regional','Human Ecology by Region or Country');
INSERT INTO concept_parent VALUES
 ('bokheim:human-ecology:general','unified:cc55bdd0-d0ae-53e1-8f70-60a68f5f2581',0,100),
 ('bokheim:human-ecology:settlements','unified:cc55bdd0-d0ae-53e1-8f70-60a68f5f2581',0,200),
 ('bokheim:human-ecology:regional','unified:cc55bdd0-d0ae-53e1-8f70-60a68f5f2581',0,300);
DELETE FROM source_selector WHERE concept_id='unified:cc55bdd0-d0ae-53e1-8f70-60a68f5f2581';
INSERT INTO source_selector VALUES
 ('bokheim:human-ecology:general','lcc','GF',0),('bokheim:human-ecology:general','lcc','GF1..GF100.9@2',1),
 ('bokheim:human-ecology:settlements','lcc','GF101..GF499.9@2',0),
 ('bokheim:human-ecology:regional','lcc','GF500..GF900@2',0);

-- 34. Teacher education.
INSERT INTO concept VALUES
 ('bokheim:teacher-education:general','Teacher Education—General'),
 ('bokheim:teacher-education:certification','Teacher Certification'),
 ('bokheim:teacher-education:profession','Teaching Profession & Vocational Guidance'),
 ('bokheim:teacher-education:us-colleges','U.S. Teachers Colleges & Normal Schools'),
 ('bokheim:teacher-education:international-colleges','Teachers Colleges & Normal Schools outside the U.S.'),
 ('bokheim:teacher-education:universities','University & College Teacher Training');
INSERT INTO concept_parent VALUES
 ('bokheim:teacher-education:general','unified:6a1b1560-81a4-569e-bf0f-0c0671e1c03e',0,100),
 ('bokheim:teacher-education:certification','unified:6a1b1560-81a4-569e-bf0f-0c0671e1c03e',0,200),
 ('bokheim:teacher-education:profession','unified:6a1b1560-81a4-569e-bf0f-0c0671e1c03e',0,300),
 ('bokheim:teacher-education:us-colleges','unified:6a1b1560-81a4-569e-bf0f-0c0671e1c03e',0,400),
 ('bokheim:teacher-education:international-colleges','unified:6a1b1560-81a4-569e-bf0f-0c0671e1c03e',0,500),
 ('bokheim:teacher-education:universities','unified:6a1b1560-81a4-569e-bf0f-0c0671e1c03e',0,600);
DELETE FROM source_selector WHERE concept_id='unified:6a1b1560-81a4-569e-bf0f-0c0671e1c03e';
INSERT INTO source_selector VALUES
 ('bokheim:teacher-education:general','lcc','LB1705..LB1770.9@3',0),
 ('bokheim:teacher-education:certification','lcc','LB1771..LB1774.9@4',0),
 ('bokheim:teacher-education:profession','lcc','LB1775..LB1804.9@4',0),
 ('bokheim:teacher-education:us-colleges','lcc','LB1805..LB1989.9@4',0),
 ('bokheim:teacher-education:international-colleges','lcc','LB1990..LB2164.9@4',0),
 ('bokheim:teacher-education:universities','lcc','LB2165..LB2286@3',0);

-- 36. Nuclear and particle physics.
INSERT INTO concept VALUES
 ('bokheim:nuclear-physics:nuclear','Nuclear Physics & Atomic Energy'),
 ('bokheim:nuclear-physics:particle','Particle Physics'),
 ('bokheim:nuclear-physics:radioactivity','Radioactivity & Radioactive Substances');
INSERT INTO concept_parent VALUES
 ('bokheim:nuclear-physics:nuclear','unified:2c271492-4980-5c66-a01c-db63d9e0bcf5',0,100),
 ('bokheim:nuclear-physics:particle','unified:2c271492-4980-5c66-a01c-db63d9e0bcf5',0,200),
 ('bokheim:nuclear-physics:radioactivity','unified:2c271492-4980-5c66-a01c-db63d9e0bcf5',0,300);
DELETE FROM source_selector WHERE concept_id='unified:2c271492-4980-5c66-a01c-db63d9e0bcf5';
INSERT INTO source_selector VALUES
 ('bokheim:nuclear-physics:nuclear','bisac','SCI051000',0),('bokheim:nuclear-physics:nuclear','lcc','QC770..QC792.9@3',0),
 ('bokheim:nuclear-physics:particle','bisac','SCI103000',0),('bokheim:nuclear-physics:particle','lcc','QC721',0),('bokheim:nuclear-physics:particle','lcc','QC793..QC794.94@3',1),
 ('bokheim:nuclear-physics:radioactivity','lcc','QC794.95..QC798@3',0);

-- 37. Gardens: exact BISAC specialties plus the LCC garden-design span.
INSERT INTO concept VALUES
 ('bokheim:gardens:design','Garden Design'),('bokheim:gardens:container','Container Gardening'),
 ('bokheim:gardens:furnishings','Garden Furnishings'),('bokheim:gardens:indoor','House Plants & Indoor Gardening'),
 ('bokheim:gardens:japanese','Japanese Gardens'),('bokheim:gardens:lawns','Lawns'),
 ('bokheim:gardens:shade','Shade Gardening'),('bokheim:gardens:urban','Urban & Community Gardening'),
 ('bokheim:gardens:water','Water Gardens');
INSERT INTO concept_parent VALUES
 ('bokheim:gardens:design','bokheim:horticulture:gardens',0,100),('bokheim:gardens:container','bokheim:horticulture:gardens',0,200),
 ('bokheim:gardens:furnishings','bokheim:horticulture:gardens',0,300),('bokheim:gardens:indoor','bokheim:horticulture:gardens',0,400),
 ('bokheim:gardens:japanese','bokheim:horticulture:gardens',0,500),('bokheim:gardens:lawns','bokheim:horticulture:gardens',0,600),
 ('bokheim:gardens:shade','bokheim:horticulture:gardens',0,700),('bokheim:gardens:urban','bokheim:horticulture:gardens',0,800),
 ('bokheim:gardens:water','bokheim:horticulture:gardens',0,900);
DELETE FROM source_selector WHERE concept_id='bokheim:horticulture:gardens';
INSERT INTO source_selector VALUES
 ('bokheim:gardens:design','bisac','GAR006000',0),('bokheim:gardens:design','lcc','SB450.9..SB467.8@2',0),
 ('bokheim:gardens:container','bisac','GAR001000',0),('bokheim:gardens:furnishings','bisac','GAR007000',0),
 ('bokheim:gardens:indoor','bisac','GAR010000',0),('bokheim:gardens:japanese','bisac','GAR013000',0),
 ('bokheim:gardens:lawns','bisac','GAR015000',0),('bokheim:gardens:shade','bisac','GAR020000',0),
 ('bokheim:gardens:urban','bisac','GAR028000',0),('bokheim:gardens:water','bisac','GAR029000',0);

-- 38. Economic development and growth.
INSERT INTO concept VALUES
 ('bokheim:economic-development:general','Economic Development—General & Reference'),
 ('bokheim:economic-development:theory','Development & Growth Theory'),
 ('bokheim:economic-development:information','Development Information & Communication'),
 ('bokheim:economic-development:research','Development Research & Education'),
 ('bokheim:economic-development:history','History of Economic Development'),
 ('bokheim:economic-development:countries','Development by Country or Region'),
 ('bokheim:economic-development:policy','Development & Economic Policy');
INSERT INTO concept_parent VALUES
 ('bokheim:economic-development:general','unified:8e51c653-486b-59e9-b73c-a63e78a420fc',0,100),
 ('bokheim:economic-development:theory','unified:8e51c653-486b-59e9-b73c-a63e78a420fc',0,200),
 ('bokheim:economic-development:information','unified:8e51c653-486b-59e9-b73c-a63e78a420fc',0,300),
 ('bokheim:economic-development:research','unified:8e51c653-486b-59e9-b73c-a63e78a420fc',0,400),
 ('bokheim:economic-development:history','unified:8e51c653-486b-59e9-b73c-a63e78a420fc',0,500),
 ('bokheim:economic-development:countries','unified:8e51c653-486b-59e9-b73c-a63e78a420fc',0,600),
 ('bokheim:economic-development:policy','unified:8e51c653-486b-59e9-b73c-a63e78a420fc',0,700);
DELETE FROM source_selector WHERE concept_id='unified:8e51c653-486b-59e9-b73c-a63e78a420fc';
INSERT INTO source_selector VALUES
 ('bokheim:economic-development:general','lcc','HD72..HD74.9@4',0),
 ('bokheim:economic-development:theory','lcc','HD75..HD75.9@4',0),
 ('bokheim:economic-development:information','lcc','HD76..HD76.9@4',0),
 ('bokheim:economic-development:research','lcc','HD77..HD77.9@4',0),
 ('bokheim:economic-development:history','lcc','HD78..HD81.9@4',0),
 ('bokheim:economic-development:countries','lcc','HD82..HD86.9@4',0),
 ('bokheim:economic-development:policy','lcc','HD87..HD88@4',0);

COMMIT;

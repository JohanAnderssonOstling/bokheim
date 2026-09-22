-- Insurance, peace studies, ethnology, library collections, and criminal law.
PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;

INSERT INTO concept(concept_id,preferred_label) VALUES
 ('bokheim:insurance:general','General Insurance, Management & Policy'),('bokheim:insurance:regional','Insurance by Region or Country'),
 ('bokheim:insurance:life','Life Insurance'),('bokheim:insurance:personal','Accident, Health & Burial Insurance'),
 ('bokheim:insurance:fire','Fire & Property Insurance'),('bokheim:insurance:transport','Transportation Insurance'),
 ('bokheim:insurance:casualty','Casualty Insurance'),('bokheim:insurance:other','Other Insurance');
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal,display_order) VALUES
 ('bokheim:insurance:general','unified:e3ef5258-7ae2-517b-9dbe-ee5304b7a926',0,100),('bokheim:insurance:regional','unified:e3ef5258-7ae2-517b-9dbe-ee5304b7a926',0,200),
 ('bokheim:insurance:life','unified:e3ef5258-7ae2-517b-9dbe-ee5304b7a926',0,300),('bokheim:insurance:personal','unified:e3ef5258-7ae2-517b-9dbe-ee5304b7a926',0,400),
 ('bokheim:insurance:fire','unified:e3ef5258-7ae2-517b-9dbe-ee5304b7a926',0,500),('bokheim:insurance:transport','unified:e3ef5258-7ae2-517b-9dbe-ee5304b7a926',0,600),
 ('bokheim:insurance:casualty','unified:e3ef5258-7ae2-517b-9dbe-ee5304b7a926',0,700),('bokheim:insurance:other','unified:e3ef5258-7ae2-517b-9dbe-ee5304b7a926',0,800);
DELETE FROM source_selector WHERE concept_id='unified:e3ef5258-7ae2-517b-9dbe-ee5304b7a926';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) VALUES
 ('bokheim:insurance:general','lcc','HG8011..HG8499.9@3',0),('bokheim:insurance:regional','lcc','HG8501..HG8745@3',0),
 ('bokheim:insurance:life','lcc','HG8751..HG9295@3',0),('bokheim:insurance:personal','lcc','HG9301..HG9649.9@3',0),
 ('bokheim:insurance:fire','lcc','HG9651..HG9899@3',0),('bokheim:insurance:transport','lcc','HG9903..HG9955.9@3',0),
 ('bokheim:insurance:casualty','lcc','HG9956..HG9969@3',0),('bokheim:insurance:other','lcc','HG9969.5..HG9999@3',0);

INSERT INTO concept(concept_id,preferred_label) VALUES
 ('bokheim:peace:general','Peace Promotion, Organizations & Biography'),('bokheim:peace:pacifism','Pacifism—History & Theory'),
 ('bokheim:peace:movements','Peace Movements, Antiwar Movements & Nonviolence'),('bokheim:peace:security','International Security, Disarmament & Global Survival'),
 ('bokheim:peace:settlement','Peaceful Settlement & Nation-Building');
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal,display_order) VALUES
 ('bokheim:peace:general','unified:b9591cf1-8997-548d-bec3-c8ead377e05e',0,100),('bokheim:peace:pacifism','unified:b9591cf1-8997-548d-bec3-c8ead377e05e',0,200),
 ('bokheim:peace:movements','unified:b9591cf1-8997-548d-bec3-c8ead377e05e',0,300),('bokheim:peace:security','unified:b9591cf1-8997-548d-bec3-c8ead377e05e',0,400),
 ('bokheim:peace:settlement','unified:b9591cf1-8997-548d-bec3-c8ead377e05e',0,500);
DELETE FROM source_selector WHERE concept_id='unified:b9591cf1-8997-548d-bec3-c8ead377e05e';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) VALUES
 ('bokheim:peace:general','bisac','POL034000',0),('bokheim:peace:general','lcc','JZ5510.2..JZ5543.9@4',0),
 ('bokheim:peace:pacifism','lcc','JZ5544..JZ5573@4',0),('bokheim:peace:movements','lcc','JZ5574..JZ5585.9@4',0),
 ('bokheim:peace:security','lcc','JZ5586..JZ6009@4',0),('bokheim:peace:settlement','lcc','JZ6010..JZ6300@4',0);

INSERT INTO concept(concept_id,preferred_label) VALUES
 ('bokheim:ethnology:general','General Ethnology'),('bokheim:ethnology:culture','Culture & Cultural Processes'),
 ('bokheim:ethnology:collections','Collected & Comparative Ethnographies'),('bokheim:ethnology:applied','Applied Anthropology'),
 ('bokheim:ethnology:traits','Cultural Traits, Customs & Institutions'),('bokheim:ethnology:groups','Ethnic Groups & Races');
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal,display_order) VALUES
 ('bokheim:ethnology:general','unified:0ccbefe9-de72-534e-8464-d95aedd9fd62',0,100),('bokheim:ethnology:culture','unified:0ccbefe9-de72-534e-8464-d95aedd9fd62',0,200),
 ('bokheim:ethnology:collections','unified:0ccbefe9-de72-534e-8464-d95aedd9fd62',0,300),('bokheim:ethnology:applied','unified:0ccbefe9-de72-534e-8464-d95aedd9fd62',0,400),
 ('bokheim:ethnology:traits','unified:0ccbefe9-de72-534e-8464-d95aedd9fd62',0,500),('bokheim:ethnology:groups','unified:0ccbefe9-de72-534e-8464-d95aedd9fd62',0,600);
DELETE FROM source_selector WHERE concept_id='unified:0ccbefe9-de72-534e-8464-d95aedd9fd62';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) VALUES
 ('bokheim:ethnology:general','lcc','GN301..GN356.9@3',0),('bokheim:ethnology:general','lcc','GN301-674',1),
 ('bokheim:ethnology:culture','lcc','GN357..GN377.9@4',0),('bokheim:ethnology:collections','lcc','GN378..GN396.9@4',0),
 ('bokheim:ethnology:applied','lcc','GN397..GN405.9@4',0),('bokheim:ethnology:traits','lcc','GN406..GN536.9@4',0),
 ('bokheim:ethnology:groups','lcc','GN537..GN674@4',0);

INSERT INTO concept(concept_id,preferred_label) VALUES
 ('bokheim:library-materials:general','Collections, Acquisition & Processing'),('bokheim:library-materials:special','Special Classes of Library Materials'),
 ('bokheim:library-materials:cataloging','Cataloging & Classification'),('bokheim:library-materials:data','Bibliographic Data & Library Automation'),
 ('bokheim:library-materials:preservation','Preservation & Conservation'),('bokheim:library-materials:services','Public, Reference & Circulation Services');
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal,display_order) VALUES
 ('bokheim:library-materials:general','unified:59ce0fe1-c05e-5de0-a3cb-1e8b853f7afd',0,100),('bokheim:library-materials:special','unified:59ce0fe1-c05e-5de0-a3cb-1e8b853f7afd',0,200),
 ('bokheim:library-materials:cataloging','unified:59ce0fe1-c05e-5de0-a3cb-1e8b853f7afd',0,300),('bokheim:library-materials:data','unified:59ce0fe1-c05e-5de0-a3cb-1e8b853f7afd',0,400),
 ('bokheim:library-materials:preservation','unified:59ce0fe1-c05e-5de0-a3cb-1e8b853f7afd',0,500),('bokheim:library-materials:services','unified:59ce0fe1-c05e-5de0-a3cb-1e8b853f7afd',0,600);
DELETE FROM source_selector WHERE concept_id='unified:59ce0fe1-c05e-5de0-a3cb-1e8b853f7afd';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) VALUES
 ('bokheim:library-materials:general','lcc','Z687..Z690.9@3',0),('bokheim:library-materials:special','lcc','Z691..Z695.99@3',0),
 ('bokheim:library-materials:cataloging','lcc','Z696..Z698.9@3',0),('bokheim:library-materials:data','lcc','Z699..Z700.89@3',0),
 ('bokheim:library-materials:preservation','lcc','Z700.9..Z710.9@3',0),('bokheim:library-materials:services','lcc','Z711..Z718.85@3',0);

INSERT INTO concept(concept_id,preferred_label) VALUES
 ('bokheim:criminal-law:general','General Criminal Law & Procedure'),('bokheim:criminal-law:canon','Canon Criminal Law'),
 ('bokheim:criminal-law:jewish','Jewish Criminal Law'),('bokheim:criminal-law:islamic','Islamic Criminal Law'),
 ('bokheim:criminal-law:british','British & Irish Criminal Law'),('bokheim:criminal-law:french','French Criminal Law'),
 ('bokheim:criminal-law:german','German Criminal Law'),('bokheim:criminal-law:us','United States Criminal Law');
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal,display_order) VALUES
 ('bokheim:criminal-law:general','unified:89b64e04-7869-53c6-9004-061a41c38948',0,100),('bokheim:criminal-law:canon','unified:89b64e04-7869-53c6-9004-061a41c38948',0,200),
 ('bokheim:criminal-law:jewish','unified:89b64e04-7869-53c6-9004-061a41c38948',0,300),('bokheim:criminal-law:islamic','unified:89b64e04-7869-53c6-9004-061a41c38948',0,400),
 ('bokheim:criminal-law:british','unified:89b64e04-7869-53c6-9004-061a41c38948',0,500),('bokheim:criminal-law:french','unified:89b64e04-7869-53c6-9004-061a41c38948',0,600),
 ('bokheim:criminal-law:german','unified:89b64e04-7869-53c6-9004-061a41c38948',0,700),('bokheim:criminal-law:us','unified:89b64e04-7869-53c6-9004-061a41c38948',0,800);
UPDATE source_selector SET concept_id='bokheim:criminal-law:general' WHERE concept_id='unified:89b64e04-7869-53c6-9004-061a41c38948' AND selector GLOB 'K[0-9]*';
UPDATE source_selector SET concept_id='bokheim:criminal-law:canon' WHERE concept_id='unified:89b64e04-7869-53c6-9004-061a41c38948' AND selector GLOB 'KB[0-9]*';
UPDATE source_selector SET concept_id='bokheim:criminal-law:jewish' WHERE concept_id='unified:89b64e04-7869-53c6-9004-061a41c38948' AND selector GLOB 'KBM*';
UPDATE source_selector SET concept_id='bokheim:criminal-law:islamic' WHERE concept_id='unified:89b64e04-7869-53c6-9004-061a41c38948' AND selector GLOB 'KBP*';
UPDATE source_selector SET concept_id='bokheim:criminal-law:british' WHERE concept_id='unified:89b64e04-7869-53c6-9004-061a41c38948' AND selector GLOB 'KI*';
UPDATE source_selector SET concept_id='bokheim:criminal-law:french' WHERE concept_id='unified:89b64e04-7869-53c6-9004-061a41c38948' AND selector GLOB 'KJ*';
UPDATE source_selector SET concept_id='bokheim:criminal-law:german' WHERE concept_id='unified:89b64e04-7869-53c6-9004-061a41c38948' AND selector GLOB 'KK*';
UPDATE source_selector SET concept_id='bokheim:criminal-law:us' WHERE concept_id='unified:89b64e04-7869-53c6-9004-061a41c38948' AND selector GLOB 'KF*';

COMMIT;

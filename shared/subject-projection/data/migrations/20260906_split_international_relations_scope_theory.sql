-- Split the JZ scope/theory schedule into five official broad divisions.
PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;
INSERT INTO concept(concept_id,preferred_label) VALUES
 ('bokheim:international-relations:general','General International Relations'),
 ('bokheim:international-relations:concepts','Concepts, Principles & Political Theory'),
 ('bokheim:international-relations:history','International Relations by Period'),
 ('bokheim:international-relations:diplomacy','Diplomatic & Consular Service'),
 ('bokheim:international-relations:regions','International Relations by Region');
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal,display_order) VALUES
 ('bokheim:international-relations:general','unified:96988a3b-c135-5c5a-9521-d8367a43419e',0,100),
 ('bokheim:international-relations:concepts','unified:96988a3b-c135-5c5a-9521-d8367a43419e',0,200),
 ('bokheim:international-relations:history','unified:96988a3b-c135-5c5a-9521-d8367a43419e',0,300),
 ('bokheim:international-relations:diplomacy','unified:96988a3b-c135-5c5a-9521-d8367a43419e',0,400),
 ('bokheim:international-relations:regions','unified:96988a3b-c135-5c5a-9521-d8367a43419e',0,500);
DELETE FROM source_selector WHERE concept_id='unified:96988a3b-c135-5c5a-9521-d8367a43419e';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) VALUES
 ('bokheim:international-relations:general','lcc','JZ1305',0),
 ('bokheim:international-relations:concepts','lcc','JZ1306..JZ1327.9@4',0),
 ('bokheim:international-relations:history','lcc','JZ1328..JZ1399.9@4',0),
 ('bokheim:international-relations:diplomacy','lcc','JZ1400..JZ1463.9@4',0),
 ('bokheim:international-relations:regions','lcc','JZ1464..JZ2060@4',0);
COMMIT;

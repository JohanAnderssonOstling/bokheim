-- Extract high-use technology topics and architectural regions.
-- Narrow ranges override the retained general/residual parent coverage.
PRAGMA foreign_keys=ON;
BEGIN;

INSERT INTO concept(concept_id,preferred_label) VALUES(51695,'Car Makes & Models');

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal) VALUES(51695,7715,0);

INSERT INTO lcc_range(class_letters,start_value,end_value,concept_id) VALUES('TL','215','215',51695);

DELETE FROM lcc_selector WHERE concept_id=7715 AND selector='TL215';

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal) VALUES(103,8486,1);

INSERT INTO lcc_range(class_letters,start_value,end_value,concept_id) VALUES('TK','5105.888','5105.8885',103);

INSERT INTO concept(concept_id,preferred_label) VALUES(51696,'Web Software');

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal) VALUES(51696,103,0);

INSERT INTO lcc_range(class_letters,start_value,end_value,concept_id) VALUES('TK','5105.8885','5105.8885',51696);

DELETE FROM lcc_selector WHERE concept_id=8486 AND selector='TK5105.8885';

INSERT INTO concept(concept_id,preferred_label) VALUES(51697,'Computational Intelligence');

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal) VALUES(51697,37,0);

INSERT INTO lcc_range(class_letters,start_value,end_value,concept_id) VALUES('Q','342','342',51697);

DELETE FROM lcc_selector WHERE concept_id=37 AND selector='Q342';

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal) VALUES(3152,12460,1);

INSERT INTO lcc_range(class_letters,start_value,end_value,concept_id) VALUES('SF','429','429',3152);

DELETE FROM lcc_selector WHERE concept_id=12460 AND selector='SF429';

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal) VALUES(272,9610,1);

INSERT INTO lcc_range(class_letters,start_value,end_value,concept_id) VALUES('TK','7871.85','7871.99',272);

INSERT INTO concept(concept_id,preferred_label) VALUES(51698,'Architectural Theory & Philosophy');

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal) VALUES(51698,5698,0);

INSERT INTO lcc_range(class_letters,start_value,end_value,concept_id) VALUES('NA','2500','2500',51698);

DELETE FROM lcc_selector WHERE concept_id=193 AND selector='NA2500';

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal) VALUES(165,8486,1);

INSERT INTO lcc_range(class_letters,start_value,end_value,concept_id) VALUES('TK','5105.59','5105.59',165);

DELETE FROM lcc_selector WHERE concept_id=8486 AND selector='TK5105.59';

INSERT INTO lcc_range(class_letters,start_value,end_value,concept_id) VALUES('T','174.7','174.7',409);

DELETE FROM lcc_selector WHERE concept_id=5563 AND selector='T174.7';

INSERT INTO concept(concept_id,preferred_label) VALUES(51699,'Architectural CAD');

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal) VALUES(51699,571,0);

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal) VALUES(51699,9867,1);

INSERT INTO lcc_range(class_letters,start_value,end_value,concept_id) VALUES('NA','2728','2728',51699);

DELETE FROM lcc_selector WHERE concept_id=193 AND selector='NA2728';

INSERT INTO concept(concept_id,preferred_label) VALUES(51700,'Construction Site Management');

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal) VALUES(51700,197,0);

UPDATE lcc_range SET concept_id=51700 WHERE class_letters='TH' AND start_value='438' AND end_value='438.4' AND concept_id=197;

INSERT INTO concept(concept_id,preferred_label) VALUES(51701,'U.S. Architecture');

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal) VALUES(51701,592,0);

INSERT INTO lcc_range(class_letters,start_value,end_value,concept_id) VALUES('NA','705','738',51701);

INSERT INTO concept(concept_id,preferred_label) VALUES(51702,'Canadian Architecture');

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal) VALUES(51702,592,0);

INSERT INTO lcc_range(class_letters,start_value,end_value,concept_id) VALUES('NA','740','749.5',51702);

INSERT INTO concept(concept_id,preferred_label) VALUES(51703,'Mexican Architecture');

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal) VALUES(51703,592,0);

INSERT INTO lcc_range(class_letters,start_value,end_value,concept_id) VALUES('NA','750','759',51703);

INSERT INTO concept(concept_id,preferred_label) VALUES(51704,'Brazilian Architecture');

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal) VALUES(51704,592,0);

INSERT INTO lcc_range(class_letters,start_value,end_value,concept_id) VALUES('NA','850','859',51704);

INSERT INTO concept(concept_id,preferred_label) VALUES(51705,'British Architecture');

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal) VALUES(51705,592,0);

INSERT INTO lcc_range(class_letters,start_value,end_value,concept_id) VALUES('NA','961','997',51705);

INSERT INTO concept(concept_id,preferred_label) VALUES(51706,'Austrian Architecture');

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal) VALUES(51706,592,0);

INSERT INTO lcc_range(class_letters,start_value,end_value,concept_id) VALUES('NA','1001','1011.6',51706);

INSERT INTO concept(concept_id,preferred_label) VALUES(51707,'Hungarian Architecture');

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal) VALUES(51707,592,0);

INSERT INTO lcc_range(class_letters,start_value,end_value,concept_id) VALUES('NA','1012','1022.6',51707);

INSERT INTO concept(concept_id,preferred_label) VALUES(51708,'Czech & Czechoslovak Architecture');

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal) VALUES(51708,592,0);

INSERT INTO lcc_range(class_letters,start_value,end_value,concept_id) VALUES('NA','1023','1034.5',51708);

INSERT INTO concept(concept_id,preferred_label) VALUES(51709,'French Architecture');

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal) VALUES(51709,592,0);

INSERT INTO lcc_range(class_letters,start_value,end_value,concept_id) VALUES('NA','1041','1053.3',51709);

INSERT INTO concept(concept_id,preferred_label) VALUES(51710,'German Architecture');

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal) VALUES(51710,592,0);

INSERT INTO lcc_range(class_letters,start_value,end_value,concept_id) VALUES('NA','1061','1088.3',51710);

INSERT INTO concept(concept_id,preferred_label) VALUES(51711,'Italian Architecture');

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal) VALUES(51711,592,0);

INSERT INTO lcc_range(class_letters,start_value,end_value,concept_id) VALUES('NA','1111','1123.3',51711);

INSERT INTO concept(concept_id,preferred_label) VALUES(51712,'Dutch Architecture');

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal) VALUES(51712,592,0);

INSERT INTO lcc_range(class_letters,start_value,end_value,concept_id) VALUES('NA','1141','1153.3',51712);

INSERT INTO concept(concept_id,preferred_label) VALUES(51713,'Belgian Architecture');

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal) VALUES(51713,592,0);

INSERT INTO lcc_range(class_letters,start_value,end_value,concept_id) VALUES('NA','1161','1173.3',51713);

INSERT INTO concept(concept_id,preferred_label) VALUES(51714,'Russian Architecture');

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal) VALUES(51714,592,0);

INSERT INTO lcc_range(class_letters,start_value,end_value,concept_id) VALUES('NA','1181','1199',51714);

INSERT INTO concept(concept_id,preferred_label) VALUES(51715,'Scandinavian Architecture');

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal) VALUES(51715,592,0);

INSERT INTO lcc_range(class_letters,start_value,end_value,concept_id) VALUES('NA','1201','1293.3',51715);

INSERT INTO concept(concept_id,preferred_label) VALUES(51716,'Portuguese Architecture');

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal) VALUES(51716,592,0);

INSERT INTO lcc_range(class_letters,start_value,end_value,concept_id) VALUES('NA','1321','1333.3',51716);

INSERT INTO concept(concept_id,preferred_label) VALUES(51717,'Swiss Architecture');

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal) VALUES(51717,592,0);

INSERT INTO lcc_range(class_letters,start_value,end_value,concept_id) VALUES('NA','1341','1353.3',51717);

INSERT INTO concept(concept_id,preferred_label) VALUES(51718,'Indian Architecture');

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal) VALUES(51718,592,0);

INSERT INTO lcc_range(class_letters,start_value,end_value,concept_id) VALUES('NA','1501','1510.3',51718);

INSERT INTO concept(concept_id,preferred_label) VALUES(51719,'Japanese Architecture');

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal) VALUES(51719,592,0);

INSERT INTO lcc_range(class_letters,start_value,end_value,concept_id) VALUES('NA','1550','1559.6',51719);

UPDATE lcc_selector SET concept_id=51719 WHERE concept_id=574 AND selector='NA1559.A5';

COMMIT;

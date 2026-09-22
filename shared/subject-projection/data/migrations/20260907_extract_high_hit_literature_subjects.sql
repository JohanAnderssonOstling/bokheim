-- Extract high-use literature topics, genres and national traditions.
-- Narrow ranges override the retained general/residual parent coverage.
PRAGMA foreign_keys=ON;
BEGIN;

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal) VALUES(4160,9098,1);

INSERT INTO lcc_range(class_letters,start_value,end_value,concept_id) VALUES('PQ','9500','9698.436',4160);

INSERT INTO concept(concept_id,preferred_label) VALUES(51864,'Galician Literature');

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal) VALUES(51864,4182,0);

INSERT INTO lcc_range(class_letters,start_value,end_value,concept_id) VALUES('PQ','9450','9469.3',51864);

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal) VALUES(4231,6797,1);

INSERT INTO lcc_range(class_letters,start_value,end_value,concept_id) VALUES('PL','201','271',4231);

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal) VALUES(614,12475,1);

INSERT INTO lcc_range(class_letters,start_value,end_value,concept_id) VALUES('PN','6790.J3','6790.J349',614);

INSERT INTO concept(concept_id,preferred_label) VALUES(51865,'South Korean Comics');

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal) VALUES(51865,12475,0);

INSERT INTO lcc_range(class_letters,start_value,end_value,concept_id) VALUES('PN','6790.K6','6790.K649',51865);

INSERT INTO concept(concept_id,preferred_label) VALUES(51866,'Croatian Literature');

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal) VALUES(51866,13657,0);

INSERT INTO lcc_range(class_letters,start_value,end_value,concept_id) VALUES('PG','1600','1696',51866);

INSERT INTO concept(concept_id,preferred_label) VALUES(51867,'Bosnian Literature');

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal) VALUES(51867,4468,0);

INSERT INTO lcc_range(class_letters,start_value,end_value,concept_id) VALUES('PG','1700','1749',51867);

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal) VALUES(4426,4265,1);

INSERT INTO lcc_range(class_letters,start_value,end_value,concept_id) VALUES('PL','2306','2333',4426);

INSERT INTO lcc_range(class_letters,start_value,end_value,concept_id) VALUES('PL','2336','2353',4426);

INSERT INTO lcc_range(class_letters,start_value,end_value,concept_id) VALUES('PL','2517','2543',4426);

INSERT INTO lcc_range(class_letters,start_value,end_value,concept_id) VALUES('PL','2548','2563',4426);

INSERT INTO concept(concept_id,preferred_label) VALUES(51868,'Chinese Drama');

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal) VALUES(51868,4265,0);

INSERT INTO lcc_range(class_letters,start_value,end_value,concept_id) VALUES('PL','2356','2393',51868);

INSERT INTO lcc_range(class_letters,start_value,end_value,concept_id) VALUES('PL','2566','2603',51868);

INSERT INTO concept(concept_id,preferred_label) VALUES(51869,'Chinese Fiction');

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal) VALUES(51869,4265,0);

INSERT INTO lcc_range(class_letters,start_value,end_value,concept_id) VALUES('PL','2415','2443',51869);

INSERT INTO lcc_range(class_letters,start_value,end_value,concept_id) VALUES('PL','2625','2653',51869);

INSERT INTO concept(concept_id,preferred_label) VALUES(51870,'Chinese Essays');

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal) VALUES(51870,4265,0);

INSERT INTO lcc_range(class_letters,start_value,end_value,concept_id) VALUES('PL','2395','2413',51870);

INSERT INTO lcc_range(class_letters,start_value,end_value,concept_id) VALUES('PL','2606','2623',51870);

INSERT INTO concept(concept_id,preferred_label) VALUES(51871,'Chinese Classics');

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal) VALUES(51871,12352,0);

INSERT INTO lcc_range(class_letters,start_value,end_value,concept_id) VALUES('PL','2458','2489.6',51871);

INSERT INTO concept(concept_id,preferred_label) VALUES(51872,'Russian Theater');

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal) VALUES(51872,12674,0);

INSERT INTO lcc_range(class_letters,start_value,end_value,concept_id) VALUES('PN','2720','2728',51872);

INSERT INTO concept(concept_id,preferred_label) VALUES(51873,'Chinese Theater');

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal) VALUES(51873,12674,0);

INSERT INTO lcc_range(class_letters,start_value,end_value,concept_id) VALUES('PN','2870','2878',51873);

INSERT INTO concept(concept_id,preferred_label) VALUES(51874,'Japanese Theater');

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal) VALUES(51874,12674,0);

INSERT INTO lcc_range(class_letters,start_value,end_value,concept_id) VALUES('PN','2920','2928',51874);

INSERT INTO concept(concept_id,preferred_label) VALUES(51875,'Indian Theater');

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal) VALUES(51875,12674,0);

INSERT INTO lcc_range(class_letters,start_value,end_value,concept_id) VALUES('PN','2880','2888',51875);

INSERT INTO concept(concept_id,preferred_label) VALUES(51876,'Spanish Theater');

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal) VALUES(51876,12674,0);

INSERT INTO lcc_range(class_letters,start_value,end_value,concept_id) VALUES('PN','2780','2788',51876);

INSERT INTO concept(concept_id,preferred_label) VALUES(51877,'Scandinavian Theater');

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal) VALUES(51877,12674,0);

INSERT INTO lcc_range(class_letters,start_value,end_value,concept_id) VALUES('PN','2730','2778',51877);

INSERT INTO concept(concept_id,preferred_label) VALUES(51878,'Italian Journalism');

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal) VALUES(51878,12675,0);

INSERT INTO lcc_range(class_letters,start_value,end_value,concept_id) VALUES('PN','5241','5250',51878);

INSERT INTO concept(concept_id,preferred_label) VALUES(51879,'Russian Journalism');

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal) VALUES(51879,12675,0);

INSERT INTO lcc_range(class_letters,start_value,end_value,concept_id) VALUES('PN','5271','5280',51879);

INSERT INTO concept(concept_id,preferred_label) VALUES(51880,'Chinese Journalism');

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal) VALUES(51880,12675,0);

INSERT INTO lcc_range(class_letters,start_value,end_value,concept_id) VALUES('PN','5361','5370',51880);

INSERT INTO concept(concept_id,preferred_label) VALUES(51881,'Indian Journalism');

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal) VALUES(51881,12675,0);

INSERT INTO lcc_range(class_letters,start_value,end_value,concept_id) VALUES('PN','5371','5380',51881);

INSERT INTO concept(concept_id,preferred_label) VALUES(51882,'Japanese Journalism');

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal) VALUES(51882,12675,0);

INSERT INTO lcc_range(class_letters,start_value,end_value,concept_id) VALUES('PN','5401','5410',51882);

INSERT INTO concept(concept_id,preferred_label) VALUES(51883,'Spanish Journalism');

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal) VALUES(51883,12675,0);

INSERT INTO lcc_range(class_letters,start_value,end_value,concept_id) VALUES('PN','5311','5320',51883);

INSERT INTO concept(concept_id,preferred_label) VALUES(51884,'Literature, Philosophy & Religion');

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal) VALUES(51884,8293,0);

INSERT INTO lcc_range(class_letters,start_value,end_value,concept_id) VALUES('PN','49','49',51884);

DELETE FROM lcc_selector WHERE concept_id=8293 AND selector='PN49';

INSERT INTO concept(concept_id,preferred_label) VALUES(51885,'Literature & Society');

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal) VALUES(51885,8293,0);

INSERT INTO lcc_range(class_letters,start_value,end_value,concept_id) VALUES('PN','51','51',51885);

DELETE FROM lcc_selector WHERE concept_id=8293 AND selector='PN51';

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal) VALUES(4590,8293,1);

INSERT INTO lcc_range(class_letters,start_value,end_value,concept_id) VALUES('PN','53','53',4590);

DELETE FROM lcc_selector WHERE concept_id=8293 AND selector='PN53';

COMMIT;

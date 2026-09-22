-- Approved context-preserving flattening: all three proposals.
-- Classification validation and usage refresh deferred.
PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;
insert into concept_parent values(11479,4582,2);
insert into concept_parent values(11481,4582,2);
insert into concept_parent values(13833,4582,3);
delete from concept_parent where parent_concept_id=4586;
delete from concept where concept_id=4586;
insert into concept_parent values(3859,3853,1);
insert into concept_parent values(3862,3853,1);
insert into concept_parent values(3863,3853,1);
delete from concept_parent where parent_concept_id=5862;
delete from concept where concept_id=5862;
insert or ignore into lcc_selector select 9056,selector from lcc_selector where concept_id=53131;
delete from lcc_selector where concept_id=53131;
insert or ignore into bisac_selector select 9056,selector from bisac_selector where concept_id=53131;
delete from bisac_selector where concept_id=53131;
update lcc_range set concept_id=9056 where concept_id=53131;
update ddc_selector set concept_id=9056 where concept_id=53131;
delete from concept_parent where concept_id=53131 or parent_concept_id=53131;
delete from concept where concept_id=53131;
update concept set preferred_label='Educational Technology' where concept_id=9056;
COMMIT;

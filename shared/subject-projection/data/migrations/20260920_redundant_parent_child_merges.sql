-- Approved redundant parent-child merges: both proposals.
-- Classification validation and usage refresh deferred.
PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;
insert or ignore into lcc_selector select 98,selector from lcc_selector where concept_id=53140;
delete from lcc_selector where concept_id=53140;
insert or ignore into bisac_selector select 98,selector from bisac_selector where concept_id=53140;
delete from bisac_selector where concept_id=53140;
update lcc_range set concept_id=98 where concept_id=53140;
update ddc_selector set concept_id=98 where concept_id=53140;
delete from concept_parent where concept_id=53140 or parent_concept_id=53140;
delete from concept where concept_id=53140;
insert into concept_parent values(562,5699,1);
insert into concept_parent values(563,5699,1);
insert into concept_parent values(564,5699,1);
insert into concept_parent values(565,5699,1);
insert or ignore into lcc_selector select 5699,selector from lcc_selector where concept_id=561;
delete from lcc_selector where concept_id=561;
insert or ignore into bisac_selector select 5699,selector from bisac_selector where concept_id=561;
delete from bisac_selector where concept_id=561;
update lcc_range set concept_id=5699 where concept_id=561;
update ddc_selector set concept_id=5699 where concept_id=561;
delete from concept_parent where concept_id=561 or parent_concept_id=561;
delete from concept where concept_id=561;
COMMIT;

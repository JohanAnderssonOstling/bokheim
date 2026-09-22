-- Promote West and South children directly to Europe; retain coded Western Europe leaf.
-- See reports/20260920-europe-subregions.json for ID mappings and source repairs.
PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;
delete from concept_parent where concept_id=3520;
insert into concept_parent values(3520,3517,0);
delete from concept_parent where concept_id=3521;
insert into concept_parent values(3521,3517,0);
delete from concept_parent where concept_id=3523;
insert into concept_parent values(3523,3517,0);
delete from concept_parent where concept_id=3535;
insert into concept_parent values(3535,3517,0);
delete from concept_parent where concept_id=3574;
insert into concept_parent values(3574,3517,0);
delete from concept_parent where concept_id=3604;
insert into concept_parent values(3604,3517,0);
delete from concept_parent where concept_id=3607;
insert into concept_parent values(3607,3517,0);
delete from concept_parent where concept_id=3614;
insert into concept_parent values(3614,3517,0);
delete from concept_parent where concept_id=51930;
insert into concept_parent values(51930,3517,0);
delete from concept_parent where concept_id=52007;
insert into concept_parent values(52007,3517,0);
update concept set preferred_label='Western Europe' where concept_id=3615;
delete from concept_parent where parent_concept_id=5541;
delete from concept where concept_id=5541;
COMMIT;

-- Social sciences sexuality disciplinary split hierarchy cleanup.
-- Classification validation and usage refresh deferred.
PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;
update concept set preferred_label='Psychology of Sexuality' where concept_id=5433;
delete from concept_parent where concept_id=5250;
insert into concept_parent values(5250,5661,0);
insert into concept_parent values(5250,54939,1);
update bisac_selector set concept_id=5433 where concept_id=5250 and selector='PSY016000';
COMMIT;

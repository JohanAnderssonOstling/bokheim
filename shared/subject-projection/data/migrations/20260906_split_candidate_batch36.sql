PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;

-- 80. Constitutional foundations and rights: comparative, religious-system, and national schedules.
INSERT INTO concept VALUES ('bokheim:constitutional-rights:comparative','Comparative Constitutional Principles & Rights'),('bokheim:constitutional-rights:religious-systems','Constitutional Principles in Religious Legal Systems'),('bokheim:constitutional-rights:national','National Constitutional Foundations & Rights');
INSERT INTO concept_parent VALUES ('bokheim:constitutional-rights:comparative','bokheim:law:public:constitutional-rights',0,100),('bokheim:constitutional-rights:religious-systems','bokheim:law:public:constitutional-rights',0,200),('bokheim:constitutional-rights:national','bokheim:law:public:constitutional-rights',0,300);
UPDATE source_selector SET concept_id='bokheim:constitutional-rights:religious-systems' WHERE concept_id='bokheim:law:public:constitutional-rights' AND system_id='lcc' AND (selector LIKE 'KBM%' OR selector LIKE 'KBU%' OR selector LIKE 'KZ%');
UPDATE source_selector SET concept_id='bokheim:constitutional-rights:comparative' WHERE concept_id='bokheim:law:public:constitutional-rights' AND (system_id='bisac' OR (system_id='lcc' AND selector GLOB 'K[0-9]*'));
UPDATE source_selector SET concept_id='bokheim:constitutional-rights:national' WHERE concept_id='bokheim:law:public:constitutional-rights';

COMMIT;

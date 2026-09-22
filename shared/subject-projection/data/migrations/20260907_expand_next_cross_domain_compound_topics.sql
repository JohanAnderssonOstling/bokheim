BEGIN;
UPDATE concept SET preferred_label='Data Structures & Abstract Data Types' WHERE concept_id=11697 AND preferred_label='Data Structures';
UPDATE concept SET preferred_label='Architectural Composition & Space' WHERE concept_id=8351 AND preferred_label='Composition. Proportion';
UPDATE concept SET preferred_label='Early-Childhood & Preschool Curricula' WHERE concept_id=6691 AND preferred_label='Curricula';
UPDATE concept SET preferred_label='Pathology by Organ System' WHERE concept_id=6409 AND preferred_label='Pathology';

CREATE TEMP TABLE _topic(label TEXT,parent_id INTEGER,selector TEXT,start_code TEXT,end_code TEXT);
INSERT INTO _topic VALUES
 ('Software Containers',180,'QA76.76.S58',NULL,NULL),('Virtual Computer Systems',180,'QA76.9.V5',NULL,NULL),
 ('Abstract Data Types',11697,'QA76.9.A23',NULL,NULL),('Data Structures',11697,'QA76.9.D35',NULL,NULL),
 ('Architectural Composition',8351,'NA2760',NULL,NULL),('Architectural Space',8351,'NA2765',NULL,NULL),
 ('Early-Childhood Curricula',6691,'LB1139.4',NULL,NULL),('Preschool Curricula',6691,'LB1140.4',NULL,NULL),
 ('Nutritional-Disease Pathology',6409,'RC620.9',NULL,NULL),
 ('Respiratory Pathology',6409,'RC711',NULL,NULL),('Digestive-System Pathology',6409,'RC802.9',NULL,NULL),
 ('Musculoskeletal Pathology',6409,'RC925.6',NULL,NULL),
 ('Applied Ethnomusicology',8448,'ML3799.2',NULL,NULL),('Ecomusicology',8448,'ML3799.3',NULL,NULL),
 ('Ethnomusicology Foundations',8448,NULL,'ML3797.6','ML3799'),
 ('Computing Exercises & Examinations',11661,'QA76.28',NULL,NULL),
 ('Transaction Processing Systems',11724,'QA76.545',NULL,NULL),
 ('Home–School Relations',2277,NULL,'LC225','LC226.7'),
 ('Parent–Teacher Associations',2277,NULL,'LC230','LC235'),
 ('Universities & Their Communities',2277,NULL,'LC237','LC238.4'),
 ('Schools as Community Centers',2277,'LC223',NULL,NULL),
 ('Place-Based Education',2277,'LC239',NULL,NULL);

DELETE FROM lcc_selector WHERE concept_id IN (180,11697,8351,6691,6409,8448,11661,11724,2277)
 AND selector IN (SELECT selector FROM _topic WHERE selector IS NOT NULL);
INSERT OR IGNORE INTO lcc_selector(concept_id,selector) VALUES
 (8448,'ML3797.6-3799'),(2277,'LC225-226.7'),(2277,'LC230-235'),(2277,'LC237-238.4');
DELETE FROM lcc_range WHERE concept_id IN (8448,2277);

INSERT INTO concept(preferred_label) SELECT DISTINCT label FROM _topic s
 WHERE NOT EXISTS(SELECT 1 FROM concept c WHERE c.preferred_label=s.label);
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT c.concept_id,s.parent_id,COALESCE((SELECT max(ordinal)+1 FROM concept_parent p WHERE p.concept_id=c.concept_id),0)
FROM (SELECT DISTINCT label,parent_id FROM _topic) s JOIN concept c ON c.preferred_label=s.label
WHERE NOT EXISTS(SELECT 1 FROM concept_parent p WHERE p.concept_id=c.concept_id AND p.parent_concept_id=s.parent_id);
INSERT OR IGNORE INTO lcc_selector(concept_id,selector)
SELECT c.concept_id,s.selector FROM _topic s JOIN concept c ON c.preferred_label=s.label WHERE s.selector IS NOT NULL;
INSERT OR IGNORE INTO lcc_range(start_code,end_code,concept_id)
SELECT s.start_code,s.end_code,c.concept_id FROM _topic s JOIN concept c ON c.preferred_label=s.label WHERE s.start_code IS NOT NULL;
DROP TABLE _topic;
COMMIT;

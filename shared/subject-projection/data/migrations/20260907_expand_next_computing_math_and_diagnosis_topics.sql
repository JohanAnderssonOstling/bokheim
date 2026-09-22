BEGIN;
CREATE TEMP TABLE _topic(label TEXT,parent_id INTEGER,selector TEXT,start_code TEXT,end_code TEXT);
INSERT INTO _topic VALUES
 ('Core Group Theory',4946,'QA171','QA174','QA183'),('Ordered Groups',4946,'QA172.4',NULL,NULL),
 ('IT Evaluation',100,'QA76.9.E95',NULL,NULL),('IT Management',100,'QA76.9.M3',NULL,NULL),
 ('Management Information Systems',100,NULL,'T58.6','T58.62'),
 ('Structured Analysis Techniques',172,'QA76.9.S84',NULL,NULL),('System Design',172,'QA76.9.S88',NULL,NULL),
 ('Coding Theory',13330,'QA268',NULL,NULL),('Switching Theory',13330,'QA268.5',NULL,NULL),
 ('Computer Architecture',59,'QA76.9.A73',NULL,NULL),('Computer Organization',59,'QA76.9.C643',NULL,NULL),
 ('Nutritional-Disease Diagnosis',8377,'RC621',NULL,NULL),
 ('Respiratory Diagnosis',8377,NULL,'RC733','RC734'),
 ('Digestive-System Diagnosis',8377,NULL,'RC803','RC805'),
 ('Musculoskeletal Diagnosis',8377,'RC925.7',NULL,NULL),
 ('Menstruation & Menopause',8390,NULL,'RG161','RG186'),
 ('Perimenopause',8390,'RG188',NULL,NULL),('Leucorrhea',8390,'RG190',NULL,NULL);

DELETE FROM lcc_selector WHERE concept_id IN (4946,100,172,13330,59,8377,8390)
 AND selector IN (SELECT selector FROM _topic WHERE selector IS NOT NULL);
INSERT OR IGNORE INTO lcc_selector(concept_id,selector) VALUES
 (4946,'QA174-183'),(100,'T58.6-58.62'),(8377,'RC733-734'),(8377,'RC803-805'),(8390,'RG161-186');
DELETE FROM lcc_range WHERE concept_id IN (4946,100,8377,8390);

-- Reusing the parent labels as child labels would create duplicate concepts.
UPDATE concept SET preferred_label='Coding & Switching Theory' WHERE concept_id=13330 AND preferred_label='Coding Theory';
UPDATE concept SET preferred_label='Computer Architecture & Organization' WHERE concept_id=59 AND preferred_label='Computer Architecture';

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

-- These exact points are already owned by the same broad leaf ranges.
DELETE FROM lcc_selector WHERE concept_id=12377 AND selector IN ('BS539','BS540','BS544');
DELETE FROM lcc_selector WHERE concept_id=12378 AND selector IN ('BS559','BS560');
DELETE FROM lcc_selector WHERE concept_id=12858 AND selector IN ('BT580','BT582.2');

DROP TABLE _topic;
COMMIT;

BEGIN;
CREATE TEMP TABLE _leaf(label TEXT,parent_id INTEGER,selector TEXT);
INSERT INTO _leaf VALUES
 ('Software Evaluation',171,'QA76.76.E93'),('Software Quality Control',171,'QA76.76.Q35'),
 ('Software Reliability',171,'QA76.76.R44'),('Software Measurement',171,'QA76.76.S65'),
 ('Software Testing',171,'QA76.76.T48'),('Software Verification & Validation',171,'QA76.76.V47'),
 ('Computers & Children',167,'QA76.9.C659'),('Computers & Society',167,'QA76.9.C66'),
 ('Computer Ethics',167,'QA76.9.M65'),('Psychology of Computing',167,'QA76.9.P75'),
 ('Computers & Women',167,'QA76.9.W65'),
 ('Computer Arithmetic',11657,'QA76.9.C62'),('Computer Mathematics',11657,'QA76.9.M35'),
 ('Computers in Mathematics',11657,'QA76.95'),
 ('Computer Science Periodicals & Societies',12684,'QA75.5'),
 ('Computer Science Dictionaries & Encyclopedias',12684,'QA76.15'),
 ('Computer Science Information Services',12684,'QA76.162'),
 ('Computer Science Writing & Authorship',12684,'QA76.165'),
 ('Computer Science Directories',12684,'QA76.215'),
 ('Computer Science for Young Readers',12684,'QA76.23'),
 ('Computer Camps',12684,'QA76.33'),('Computer Laboratories',12684,'QA76.35');

DELETE FROM lcc_selector WHERE concept_id IN (171,167,11657,12684);
-- General works and essays stay directly on the resource parent.
INSERT OR IGNORE INTO lcc_selector(concept_id,selector) VALUES
 (12684,'QA76.16'),(12684,'QA76.24');
INSERT INTO concept(preferred_label)
SELECT DISTINCT label FROM _leaf s WHERE NOT EXISTS(SELECT 1 FROM concept c WHERE c.preferred_label=s.label);
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT c.concept_id,s.parent_id,COALESCE((SELECT max(ordinal)+1 FROM concept_parent p WHERE p.concept_id=c.concept_id),0)
FROM _leaf s JOIN concept c ON c.preferred_label=s.label
WHERE NOT EXISTS(SELECT 1 FROM concept_parent p WHERE p.concept_id=c.concept_id AND p.parent_concept_id=s.parent_id);
INSERT OR IGNORE INTO lcc_selector(concept_id,selector)
SELECT c.concept_id,s.selector FROM _leaf s JOIN concept c ON c.preferred_label=s.label;
DROP TABLE _leaf;
COMMIT;

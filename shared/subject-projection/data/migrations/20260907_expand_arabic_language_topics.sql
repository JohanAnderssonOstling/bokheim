BEGIN;
CREATE TEMP TABLE _arabic(label TEXT,parent_id INTEGER,selector TEXT);
INSERT INTO _arabic VALUES
 ('Early Arabic Grammar Treatises',12867,'PJ6101'),
 ('Modern Arabic Grammar Treatises',12867,'PJ6106'),
 ('Arabic Language Textbooks',12867,'PJ6111'),
 ('Arabic Conversation & Phrasebooks',12867,'PJ6115'),
 ('Arabic Readers',12867,'PJ6119'),('Technical Arabic',12867,'PJ6119.5'),
 ('Arabic Language Examinations',12867,'PJ6120'),
 ('Arabic Morphology',12869,'PJ6131'),('Arabic Syntax',12869,'PJ6151'),
 ('Arabic Rhetoric',12869,'PJ6161'),('Arabic Vocabulary & Word Choice',12869,'PJ6166'),
 ('Arabic Idioms & Usage',12869,'PJ6167'),('Arabic Translation',12869,'PJ6170'),
 ('Arabic Metrics & Versification',12869,'PJ6171');

DELETE FROM lcc_selector WHERE concept_id IN (12867,12869);
INSERT OR IGNORE INTO lcc_selector(concept_id,selector) VALUES
 (12867,'PJ6073-6120.9'),(12869,'PJ6131-6171.9');
DELETE FROM lcc_range WHERE concept_id IN (12867,12869);
INSERT INTO concept(preferred_label) SELECT DISTINCT label FROM _arabic s
 WHERE NOT EXISTS(SELECT 1 FROM concept c WHERE c.preferred_label=s.label);
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT c.concept_id,s.parent_id,COALESCE((SELECT max(ordinal)+1 FROM concept_parent p WHERE p.concept_id=c.concept_id),0)
FROM (SELECT DISTINCT label,parent_id FROM _arabic) s JOIN concept c ON c.preferred_label=s.label
WHERE NOT EXISTS(SELECT 1 FROM concept_parent p WHERE p.concept_id=c.concept_id AND p.parent_concept_id=s.parent_id);
INSERT OR IGNORE INTO lcc_selector(concept_id,selector)
SELECT c.concept_id,s.selector FROM _arabic s JOIN concept c ON c.preferred_label=s.label;
DROP TABLE _arabic;
COMMIT;

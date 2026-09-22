BEGIN;
CREATE TEMP TABLE _leaf(label TEXT,parent_id INTEGER);
CREATE TEMP TABLE _exact(label TEXT,selector TEXT);
CREATE TEMP TABLE _range(label TEXT,start_code TEXT,end_code TEXT);
INSERT INTO _leaf VALUES
 ('General Folk Literature',5065),('Folk-Literature Forms',5065),('Memorates',5065),
 ('Economic Impact of Defense & Disarmament',13621),('Economic Impact of Disasters',13621),
 ('Operational Standardization & Waste Reduction',52399),
 ('Operational Support Services',52399),('Workflow Management',52399);
INSERT INTO _exact VALUES
 ('Memorates','GR79.5'),('Memorates','GR79.6'),
 ('Economic Impact of Defense & Disarmament','HC79.D4'),('Economic Impact of Disasters','HC79.D45'),
 ('Operational Standardization & Waste Reduction','HD62'),('Workflow Management','HD62.17');
INSERT INTO _range VALUES
 ('General Folk Literature','GR72','GR73.19'),('Folk-Literature Forms','GR73.2','GR79.222'),
 ('Memorates','GR79.5','GR79.6'),('Operational Support Services','HD62.12','HD62.13');

DELETE FROM lcc_selector WHERE concept_id IN (5065,13621,52399);
DELETE FROM lcc_range WHERE concept_id IN (5065,52399);
INSERT OR IGNORE INTO lcc_selector(concept_id,selector) VALUES
 (5065,'GR72-79'),(5065,'GR73.2-79.222'),(52399,'HD62.12-62.13');
INSERT INTO concept(preferred_label) SELECT DISTINCT label FROM _leaf s
 WHERE NOT EXISTS(SELECT 1 FROM concept c WHERE c.preferred_label=s.label);
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT c.concept_id,s.parent_id,COALESCE((SELECT max(ordinal)+1 FROM concept_parent p WHERE p.concept_id=c.concept_id),0)
FROM _leaf s JOIN concept c ON c.preferred_label=s.label
WHERE NOT EXISTS(SELECT 1 FROM concept_parent p WHERE p.concept_id=c.concept_id AND p.parent_concept_id=s.parent_id);
INSERT OR IGNORE INTO lcc_selector(concept_id,selector)
 SELECT c.concept_id,s.selector FROM _exact s JOIN concept c ON c.preferred_label=s.label;
INSERT OR IGNORE INTO lcc_range(start_code,end_code,concept_id)
 SELECT s.start_code,s.end_code,c.concept_id FROM _range s JOIN concept c ON c.preferred_label=s.label;

-- Generated punctuation variants are not separate LCC classifications.
DELETE FROM lcc_selector WHERE concept_id=12438 AND selector IN ('HN90.C6+','HN90.C6;');
DROP TABLE _range; DROP TABLE _exact; DROP TABLE _leaf;
COMMIT;

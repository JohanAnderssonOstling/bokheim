BEGIN;
CREATE TEMP TABLE _leaf(label TEXT NOT NULL,parent_id INTEGER NOT NULL);
CREATE TEMP TABLE _exact(label TEXT NOT NULL,selector TEXT NOT NULL);
CREATE TEMP TABLE _range(label TEXT NOT NULL,start_code TEXT NOT NULL,end_code TEXT NOT NULL);

INSERT INTO _leaf VALUES
 ('General Intelligence Services',5342),('U.S. Intelligence & Espionage',5342),
 ('Technological Innovation Management',11591),('Intellectual Capital Management',11591),
 ('Scientific Family Research',11802),('Family Demography',11802),
 ('Sociology Study & Research',12321),('Sociology Textbooks & Syllabi',12321),
 ('Social Science Research — General',12246),('U.S. Social Science Research',12246),
 ('Industrial Costs',13620),('Industrial Location',13620),('Industrial Productivity',13620),
 ('Interindustry Economics',13620),('Inventory Economics',13620),('Labor Productivity',13620);

INSERT INTO _exact VALUES
 ('General Intelligence Services','JF1525.I6'),('U.S. Intelligence & Espionage','JK468.I6'),
 ('Technological Innovation Management','HD45'),
 ('Intellectual Capital Management','HD53'),
 ('Scientific Family Research','HQ728'),('Family Demography','HQ759.98'),
 ('Sociology Textbooks & Syllabi','HM66'),('Sociology Textbooks & Syllabi','HM68'),
 ('Social Science Research — General','H62'),('U.S. Social Science Research','H62.5.U5'),
 ('Industrial Costs','HC79.C7'),('Industrial Location','HC79.D5'),
 ('Industrial Productivity','HC79.I52'),('Interindustry Economics','HC79.I57'),
 ('Inventory Economics','HC79.I6'),('Labor Productivity','HC79.L3');

INSERT INTO _range VALUES
 ('Technological Innovation Management','HD45','HD45.2'),
 ('Sociology Study & Research','HM45','HM48'),
 ('Sociology Study & Research','HM571','HM578');

DELETE FROM lcc_selector WHERE concept_id IN (5342,11591,11802,12321,12246,13620);
DELETE FROM lcc_range WHERE concept_id IN (11591,12321);
INSERT OR IGNORE INTO lcc_selector(concept_id,selector) VALUES
 (44,'HD38.7'),(11591,'HD45-45.2'),(12321,'HM45-48'),(12321,'HM571-578');

INSERT INTO concept(preferred_label)
SELECT DISTINCT label FROM _leaf s WHERE NOT EXISTS(SELECT 1 FROM concept c WHERE c.preferred_label=s.label);
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT c.concept_id,s.parent_id,COALESCE((SELECT max(ordinal)+1 FROM concept_parent p WHERE p.concept_id=c.concept_id),0)
FROM _leaf s JOIN concept c ON c.preferred_label=s.label
WHERE NOT EXISTS(SELECT 1 FROM concept_parent p WHERE p.concept_id=c.concept_id AND p.parent_concept_id=s.parent_id);
INSERT OR IGNORE INTO lcc_selector(concept_id,selector)
SELECT c.concept_id,s.selector FROM _exact s JOIN concept c ON c.preferred_label=s.label;
INSERT OR IGNORE INTO lcc_range(start_code,end_code,concept_id)
SELECT s.start_code,s.end_code,c.concept_id FROM _range s JOIN concept c ON c.preferred_label=s.label;

-- Clean the Asian-government residual without turning its valid broad leaf
-- into an overlapping hierarchy. JQ745 is Singapore; JQ3500-3509 is German
-- East Africa. The smaller JQ ranges were duplicates inside JQ21-199.99.
DELETE FROM lcc_selector WHERE concept_id=5367 AND selector IN ('JQ','JQ745');
DELETE FROM lcc_range WHERE concept_id=5367 AND start_code IN ('JQ31','JQ36','JQ94','JQ3500');
INSERT INTO concept(preferred_label)
SELECT 'Singapore Government' WHERE NOT EXISTS(SELECT 1 FROM concept WHERE preferred_label='Singapore Government');
INSERT INTO concept(preferred_label)
SELECT 'German East African Government' WHERE NOT EXISTS(SELECT 1 FROM concept WHERE preferred_label='German East African Government');
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT c.concept_id,11904,COALESCE((SELECT max(ordinal)+1 FROM concept_parent p WHERE p.concept_id=c.concept_id),0)
FROM concept c WHERE c.preferred_label='Singapore Government'
AND NOT EXISTS(SELECT 1 FROM concept_parent p WHERE p.concept_id=c.concept_id AND p.parent_concept_id=11904);
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT c.concept_id,11884,COALESCE((SELECT max(ordinal)+1 FROM concept_parent p WHERE p.concept_id=c.concept_id),0)
FROM concept c WHERE c.preferred_label='German East African Government'
AND NOT EXISTS(SELECT 1 FROM concept_parent p WHERE p.concept_id=c.concept_id AND p.parent_concept_id=11884);
INSERT OR IGNORE INTO lcc_selector(concept_id,selector)
SELECT concept_id,'JQ745' FROM concept WHERE preferred_label='Singapore Government';
INSERT OR IGNORE INTO lcc_range(start_code,end_code,concept_id)
SELECT 'JQ3500','JQ3509',concept_id FROM concept WHERE preferred_label='German East African Government';

DROP TABLE _range; DROP TABLE _exact; DROP TABLE _leaf;
COMMIT;

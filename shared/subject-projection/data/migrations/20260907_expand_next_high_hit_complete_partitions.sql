BEGIN;
CREATE TEMP TABLE _split(label TEXT,parent_id INTEGER,start_code TEXT,end_code TEXT);
INSERT INTO _split VALUES
 ('Crisis & Emergency Management',11592,'HD49','HD49.6'),
 ('Industrial Risk Management',11592,'HD61','HD61.5'),
 ('Photographic Collections through 1900',12571,'TR650','TR652'),
 ('Photographic Collections, 1901–1950',12571,'TR653','TR653'),
 ('Photographic Collections, 1951–2000',12571,'TR654','TR654'),
 ('Photographic Collections since 2001',12571,'TR655','TR655'),
 ('English Grammar Textbooks',12218,'PE1108','PE1113.99'),
 ('English Grammar Quizzes & Examinations',12218,'PE1114','PE1114'),
 ('Branch & Family-Business Management',52740,'HD62.2','HD62.34'),
 ('Government Enterprise Management',52740,'HD62.35','HD62.49'),
 ('Startup & Business-Incubator Management',52740,'HD62.5','HD62.59'),
 ('Nonprofit Management',52740,'HD62.6','HD62.69'),
 ('Small-Business Management',52740,'HD62.7','HD62.8');

INSERT OR IGNORE INTO lcc_selector(concept_id,selector)
SELECT concept_id,start_code||'-'||end_code FROM lcc_range
WHERE concept_id IN (11592,12571,12218,52740);
DELETE FROM lcc_range WHERE concept_id IN (11592,12571,12218,52740);
INSERT INTO concept(preferred_label) SELECT DISTINCT label FROM _split s
WHERE NOT EXISTS(SELECT 1 FROM concept c WHERE c.preferred_label=s.label);
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT c.concept_id,s.parent_id,COALESCE((SELECT max(ordinal)+1 FROM concept_parent p WHERE p.concept_id=c.concept_id),0)
FROM (SELECT DISTINCT label,parent_id FROM _split) s JOIN concept c ON c.preferred_label=s.label
WHERE NOT EXISTS(SELECT 1 FROM concept_parent p WHERE p.concept_id=c.concept_id AND p.parent_concept_id=s.parent_id);
INSERT OR IGNORE INTO lcc_range(start_code,end_code,concept_id)
SELECT s.start_code,s.end_code,c.concept_id FROM _split s JOIN concept c ON c.preferred_label=s.label;
DROP TABLE _split;
COMMIT;

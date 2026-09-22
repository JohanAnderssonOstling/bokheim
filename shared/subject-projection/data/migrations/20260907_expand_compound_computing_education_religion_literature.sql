BEGIN;
UPDATE concept SET preferred_label='Object-Oriented Computing' WHERE concept_id=159 AND preferred_label='Object Oriented';
UPDATE concept SET preferred_label='Technology in Education' WHERE concept_id=9056 AND preferred_label='Educational Technology';
UPDATE concept SET preferred_label='African American Literary Criticism' WHERE concept_id=4312 AND preferred_label='African American & Black';
UPDATE concept SET preferred_label='Film Biography' WHERE concept_id=12964 AND preferred_label='Film Biography & Directories';

CREATE TEMP TABLE _topic(label TEXT,parent_id INTEGER,selector TEXT);
INSERT INTO _topic VALUES
 ('Object-Oriented Programming',159,'QA76.64'),('Object-Oriented Methods',159,'QA76.9.O35'),
 ('Educational Technology',9056,'LB1028.3'),('Robotics in Education',9056,'LB1028.76'),
 ('Hypertext Systems',13158,'QA76.76.H94'),('Interactive Media & Hypermedia',13158,'QA76.76.I59'),
 ('Religion & Politics',51521,'BL65.P7'),('Religion & the State',51521,'BL65.S8'),
 ('African American Authors — Criticism',4312,'PS153.N5'),
 ('African American Fiction — Criticism',4312,'PS374.N4'),
 ('Human Factors in Software',98,'QA76.76.H85'),
 ('Human-Computer Interaction Studies',98,'QA76.9.H85');

DELETE FROM lcc_selector WHERE concept_id IN (159,9056,13158,51521,4312,98)
 AND selector IN (SELECT selector FROM _topic);
INSERT INTO concept(preferred_label) SELECT DISTINCT label FROM _topic s
 WHERE NOT EXISTS(SELECT 1 FROM concept c WHERE c.preferred_label=s.label);
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT c.concept_id,s.parent_id,COALESCE((SELECT max(ordinal)+1 FROM concept_parent p WHERE p.concept_id=c.concept_id),0)
FROM (SELECT DISTINCT label,parent_id FROM _topic) s JOIN concept c ON c.preferred_label=s.label
WHERE NOT EXISTS(SELECT 1 FROM concept_parent p WHERE p.concept_id=c.concept_id AND p.parent_concept_id=s.parent_id);
INSERT OR IGNORE INTO lcc_selector(concept_id,selector)
SELECT c.concept_id,s.selector FROM _topic s JOIN concept c ON c.preferred_label=s.label;

DELETE FROM lcc_selector WHERE concept_id=72 AND selector='QA76.9D3';
DROP TABLE _topic;
COMMIT;

BEGIN;
UPDATE concept SET preferred_label='Abstract Algebra'
WHERE concept_id=4918 AND preferred_label='Abstract';

CREATE TEMP TABLE _topic(label TEXT,parent_id INTEGER,selector TEXT,start_code TEXT,end_code TEXT);
INSERT INTO _topic VALUES
 ('Homological Algebra',4918,'QA169',NULL,NULL),
 ('Ordered Algebraic Structures',4918,'QA172',NULL,NULL),
 ('Universal Algebra',4918,NULL,'QA251','QA252.5'),
 ('Classical Differential Geometry',6518,NULL,'QA641','QA670'),
 ('Global Riemannian Geometry',6518,'QA671',NULL,NULL),
 ('Spin Geometry',6518,'QA671.5',NULL,NULL),
 ('Integral Geometry',6518,'QA672',NULL,NULL),
 ('Microsoft Windows',147,'QA76.76.W56',NULL,NULL),
 ('Windows 8',147,'QA76.774.M434',NULL,NULL),
 ('Windows 10',147,'QA76.774.M435',NULL,NULL);

DELETE FROM lcc_selector WHERE concept_id IN (4918,6518,147)
 AND selector IN (SELECT selector FROM _topic WHERE selector IS NOT NULL);
INSERT OR IGNORE INTO lcc_selector(concept_id,selector) VALUES
 (4918,'QA251-252.5'),(6518,'QA641-670');
DELETE FROM lcc_range WHERE concept_id IN (4918,6518);

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

BEGIN;
UPDATE concept SET preferred_label='Databases & Web Databases' WHERE concept_id=46 AND preferred_label='Databases';
UPDATE concept SET preferred_label='Computer Literacy & End-User Skills' WHERE concept_id=61 AND preferred_label='Computer Literacy';
UPDATE concept SET preferred_label='Gender Humor' WHERE concept_id=713 AND preferred_label='Men, Women & Relationships';
UPDATE concept SET preferred_label='Solar Cells' WHERE concept_id=6855 AND preferred_label='Solar batteries. Solar cells';
UPDATE concept SET preferred_label='Blood Examination & Collection' WHERE concept_id=9358 AND preferred_label='Examination of the blood';
UPDATE concept SET preferred_label='Photographic Exposure & Flash' WHERE concept_id=9501 AND preferred_label='Exposures';
UPDATE concept SET preferred_label='Number-Theory Arithmetic & Geometry' WHERE concept_id=13332 AND preferred_label='Integer Arithmetic';
UPDATE concept SET preferred_label='Space Environment & Simulation Engineering' WHERE concept_id=9112 AND preferred_label='Space Environmental Engineering';

CREATE TEMP TABLE _topic(label TEXT,parent_id INTEGER,selector TEXT,start_code TEXT,end_code TEXT);
INSERT INTO _topic VALUES
 ('Databases',46,'QA76.9.D32',NULL,NULL),('Web Databases',46,'QA76.9.W43',NULL,NULL),
 ('Computer Literacy',61,'QA76.9.C64',NULL,NULL),('End-User Computing',61,'QA76.9.E53',NULL,NULL),
 ('Keyboarding',61,'QA76.9.K48',NULL,NULL),
 ('Humor about Men',713,'PN6231.M45',NULL,NULL),('Humor about Women',713,'PN6231.W6',NULL,NULL),
 ('Perovskite Solar Cells',6855,'TK2963.P47',NULL,NULL),
 ('Space Environmental Engineering',9112,NULL,'TL1489','TL1565'),
 ('Space Test & Training Equipment',9112,'TL1570',NULL,NULL),
 ('Space-Cabin Simulators',9112,'TL1572',NULL,NULL),('Weightlessness Simulators',9112,'TL1575',NULL,NULL),
 ('Blood Examination',9358,'RB45',NULL,NULL),('Phlebotomy & Blood Collection',9358,'RB45.15',NULL,NULL),
 ('Photographic Exposure',9501,NULL,'TR591','TR600'),('Flash Photography',9501,'TR605',NULL,NULL),
 ('Electronic Flash',9501,'TR606',NULL,NULL),
 ('Computer Capacity',11713,'QA76.9.C63',NULL,NULL),
 ('Computer Performance Evaluation',11713,'QA76.9.E94',NULL,NULL),
 ('Divisibility, Congruences & Factorization',13332,'QA242',NULL,NULL),
 ('Arithmetical Algebraic Geometry',13332,'QA242.5',NULL,NULL),('Arakelov Theory',13332,'QA242.6',NULL,NULL);

DELETE FROM lcc_selector WHERE concept_id IN (46,61,713,6855,9112,9358,9501,11713,13332)
 AND selector IN (SELECT selector FROM _topic WHERE selector IS NOT NULL);
INSERT OR IGNORE INTO lcc_selector(concept_id,selector) VALUES
 (9112,'TL1489-1565'),(9501,'TR591-600');
DELETE FROM lcc_range WHERE concept_id IN (9112,9501);

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

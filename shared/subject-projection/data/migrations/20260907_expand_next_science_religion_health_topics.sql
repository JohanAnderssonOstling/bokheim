BEGIN;
UPDATE concept SET preferred_label='Approximation Theory'
WHERE concept_id=4957 AND preferred_label='Numerical Analysis';

CREATE TEMP TABLE _topic(label TEXT,parent_id INTEGER,selector TEXT,start_code TEXT,end_code TEXT);
INSERT INTO _topic VALUES
 ('Radial Basis Functions',4957,'QA223',NULL,NULL),('Spline Theory',4957,'QA224',NULL,NULL),
 ('Chromatography',8273,'RB40.7',NULL,NULL),('Electrochemical Analysis',8273,'RB41',NULL,NULL),
 ('Religion & Heterosexism',51522,'BL65.H47',NULL,NULL),
 ('Religion & Homosexuality',51522,'BL65.H64',NULL,NULL),
 ('Religion, Sex & Gender Roles',51522,'BL65.S4',NULL,NULL),
 ('Jewish Responses to Christianity',5874,'BM590',NULL,NULL),
 ('Jewish Responses to Islam',5874,'BM591',NULL,NULL),
 ('Heresy in Judaism',5874,'BM646',NULL,NULL),('Jewish Apologetics',5874,'BM648',NULL,NULL),
 ('Pollution-Control Fundamentals',7855,NULL,'TD191.5','TD192.7'),
 ('Fungal Remediation',7855,'TD192.72',NULL,NULL),
 ('Phytoremediation',7855,'TD192.75',NULL,NULL),('In Situ Remediation',7855,'TD192.8',NULL,NULL),
 ('Foundations of Dynamics',8766,NULL,'QC133','QC136'),('Inertia',8766,'QC137',NULL,NULL),
 ('Velocity & Speed',8766,'QC137.5',NULL,NULL),('Velocity & Speed',8766,'QC137.52',NULL,NULL);

DELETE FROM lcc_selector WHERE concept_id IN (4957,8273,51522,5874,7855,8766)
 AND selector IN (SELECT selector FROM _topic WHERE selector IS NOT NULL);
INSERT OR IGNORE INTO lcc_selector(concept_id,selector) VALUES
 (5874,'BM585-585.4'),(7855,'TD191.5-192.7'),(8766,'QC133-136');
DELETE FROM lcc_range WHERE concept_id IN (5874,7855,8766);

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

-- These exact selectors duplicate the same leaf's permissive range and made
-- broad, intentionally flattened religion/travel leaves look splittable.
DELETE FROM lcc_selector WHERE concept_id=12374 AND selector IN
 ('BS410','BS411','BS415.3','BS415.5','BS416','BS417','BS418','BS419','BS432','BS435','BS440');
DELETE FROM lcc_selector WHERE concept_id=12855 AND selector IN
 ('BT198','BT199','BT199.5','BT205','BT207','BT210','BT222','BT225','BT295');
DELETE FROM lcc_selector WHERE concept_id=12380 AND selector IN
 ('BS615','BS617.7','BS617.8','BS618','BS619');
DELETE FROM lcc_selector WHERE concept_id=13016 AND selector IN ('BQ4155','BQ4160','BQ4165','BQ4170');
DELETE FROM lcc_selector WHERE concept_id=13019 AND selector IN
 ('BQ4620','BQ4625','BQ4630','BQ4635','BQ4640','BQ4645','BQ4648','BQ4650');
DELETE FROM lcc_selector WHERE concept_id=2486 AND selector IN
 ('DT45','DT49.98','DT50','DT51','DT53','DT54','DT55','DT56','DT56.2');

DROP TABLE _topic;
COMMIT;

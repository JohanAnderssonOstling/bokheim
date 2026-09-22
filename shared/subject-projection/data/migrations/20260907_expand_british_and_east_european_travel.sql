BEGIN;
CREATE TEMP TABLE _topic(label TEXT,parent_id INTEGER,start_code TEXT,end_code TEXT,selector TEXT);
INSERT INTO _topic VALUES
 ('England Travel',2461,'DA600','DA667',NULL),
 ('Wales Travel',2461,'DA725','DA738',NULL),
 ('Scotland Travel',2461,'DA850','DA878',NULL),
 ('Edinburgh Travel',2461,'DA890.E3A2','DA890.E3Z',NULL),
 ('Commonwealth Travel',2461,NULL,NULL,'DA11'),
 ('London Travel',2461,NULL,NULL,'DA679'),
 ('Czechoslovak Travel',13996,'DB2000','DB2035',NULL),
 ('Prague Travel',13996,NULL,NULL,'DB2607'),
 ('Hungarian Travel',13996,'DB906.9','DB918.99',NULL),
 ('Budapest Travel',13996,NULL,NULL,'DB983.5'),
 ('Black Sea Region Travel',13996,NULL,NULL,'DJK63'),
 ('Carpathian Region Travel',13996,NULL,NULL,'DJK73'),
 ('Baltic States Travel',13996,NULL,NULL,'DK502.4'),
 ('Eastern Europe Travel — General',13996,'DJK13','DJK19',NULL),
 ('Former Yugoslavia Travel',13996,'DR1220','DR1224',NULL);

DELETE FROM lcc_selector WHERE concept_id IN (2461,13996)
 AND selector IN (SELECT selector FROM _topic WHERE selector IS NOT NULL);
DELETE FROM lcc_range WHERE concept_id IN (2461,13996);
INSERT OR IGNORE INTO lcc_selector(concept_id,selector) VALUES
 (2461,'DA600-667'),(2461,'DA725-738'),(2461,'DA850-878'),
 (2461,'DA890.E3A2-DA890.E3Z'),(13996,'DB2000-2035'),
 (13996,'DB906.9-920.5'),(13996,'DJK13-19'),(13996,'DR1220-1224');

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

-- DB919-919.2 is Hungarian ethnography, not description or travel.
INSERT INTO concept(preferred_label)
SELECT 'Hungarian Ethnography' WHERE NOT EXISTS(SELECT 1 FROM concept WHERE preferred_label='Hungarian Ethnography');
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT c.concept_id,5062,COALESCE((SELECT max(ordinal)+1 FROM concept_parent p WHERE p.concept_id=c.concept_id),0)
FROM concept c WHERE c.preferred_label='Hungarian Ethnography'
AND NOT EXISTS(SELECT 1 FROM concept_parent p WHERE p.concept_id=c.concept_id AND p.parent_concept_id=5062);
INSERT OR IGNORE INTO lcc_range(start_code,end_code,concept_id)
SELECT 'DB919','DB919.2',concept_id FROM concept WHERE preferred_label='Hungarian Ethnography';

DROP TABLE _topic;
COMMIT;

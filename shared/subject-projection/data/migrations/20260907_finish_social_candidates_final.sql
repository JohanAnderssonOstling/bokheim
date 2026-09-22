BEGIN;

-- Social psychology: exact umbrella spellings on the parent, numeric topics
-- on existing or newly created leaves.
INSERT OR IGNORE INTO lcc_selector(concept_id,selector)
SELECT concept_id,start_code||'-'||substr(end_code,length(rtrim(end_code,'0123456789.'))+1)
FROM lcc_range WHERE concept_id=5518;
DELETE FROM lcc_range WHERE concept_id=5518;

WITH wanted(label) AS (VALUES
 ('Social-Psychology Research'),('Aggression & Social Conflict'),
 ('Alienation & Social Isolation'),('Help-Seeking Behavior'),
 ('Interpersonal Attraction'))
INSERT INTO concept(preferred_label) SELECT label FROM wanted
WHERE NOT EXISTS(SELECT 1 FROM concept WHERE preferred_label=label);
INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT concept_id,5518,0 FROM concept WHERE preferred_label IN (
 'Social-Psychology Research','Aggression & Social Conflict',
 'Alienation & Social Isolation','Help-Seeking Behavior','Interpersonal Attraction');

INSERT OR IGNORE INTO lcc_range(start_code,end_code,concept_id) VALUES
 ('HM1013','HM1017',9965),('HM1025','HM1027',9170),
 ('HM1029','HM1031',9904),('HM1041','HM1101',8235),
 ('HM1106','HM1171',8858);
INSERT OR IGNORE INTO lcc_range(start_code,end_code,concept_id)
SELECT 'HM1019','HM1023',concept_id FROM concept WHERE preferred_label='Social-Psychology Research';
INSERT OR IGNORE INTO lcc_range(start_code,end_code,concept_id)
SELECT 'HM1116','HM1126',concept_id FROM concept WHERE preferred_label='Aggression & Social Conflict';
INSERT OR IGNORE INTO lcc_range(start_code,end_code,concept_id)
SELECT 'HM1131','HM1136',concept_id FROM concept WHERE preferred_label='Alienation & Social Isolation';
INSERT OR IGNORE INTO lcc_range(start_code,end_code,concept_id)
SELECT 'HM1141','HM1146',concept_id FROM concept WHERE preferred_label='Help-Seeking Behavior';
INSERT OR IGNORE INTO lcc_range(start_code,end_code,concept_id)
SELECT 'HM1151','HM1161',concept_id FROM concept WHERE preferred_label='Interpersonal Attraction';

-- Replace the obsolete synthetic JX bucket with modern destinations.
INSERT OR IGNORE INTO lcc_selector(concept_id,selector) VALUES
 (5293,'JX1-1620.9'),(5293,'JX1897-6650');
DELETE FROM lcc_range WHERE concept_id=13772;

INSERT OR IGNORE INTO lcc_range(start_code,end_code,concept_id) VALUES
 ('JX1','JX91',13228),('JX101','JX115',5347),
 ('JX118','JX182',5346),('JX220','JX1195',12893),
 ('JX1261','JX1283',5293),('JX1305','JX1395',12891),
 ('JX1404','JX1598',12893),('JX1621','JX1896',5347),
 ('JX2001','JX3695',5293),('JX4000','JX4084',5293),
 ('JX4085','JX4157.2',13230),('JX4471','JX5397',13233),
 ('JX5401','JX5751',5293),('JX6001','JX6650',5324);

INSERT INTO concept(preferred_label) SELECT 'International Arbitration & Organization'
WHERE NOT EXISTS(SELECT 1 FROM concept WHERE preferred_label='International Arbitration & Organization');
INSERT OR IGNORE INTO lcc_range(start_code,end_code,concept_id)
SELECT 'JX1901','JX1995',concept_id FROM concept
WHERE preferred_label='International Arbitration & Organization';
INSERT INTO concept(preferred_label) SELECT 'International Jurisdiction & Citizenship'
WHERE NOT EXISTS(SELECT 1 FROM concept WHERE preferred_label='International Jurisdiction & Citizenship');
INSERT OR IGNORE INTO lcc_range(start_code,end_code,concept_id)
SELECT 'JX4173','JX4449',concept_id FROM concept
WHERE preferred_label='International Jurisdiction & Citizenship';

-- Attach the two new leaves beneath International Law before deleting the
-- obsolete source node.
INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT concept_id,5293,0 FROM concept WHERE preferred_label IN (
 'International Arbitration & Organization','International Jurisdiction & Citizenship');
DELETE FROM source_label WHERE concept_id=13772;
DELETE FROM lcc_selector WHERE concept_id=13772;
DELETE FROM bisac_selector WHERE concept_id=13772;
DELETE FROM concept_parent WHERE concept_id=13772 OR parent_concept_id=13772;
DELETE FROM concept WHERE concept_id=13772;

COMMIT;

BEGIN;

-- Moldova follows the same explicit LCC thematic layout as Belarus.
WITH wanted(label) AS (VALUES
 ('Moldovan Antiquities'),('Moldovan Social & Cultural History'),
 ('Moldovan Ethnography'),('Moldovan Historical Biographies'),
 ('Moldovan Historiography'),('Moldovan Military History'),
 ('Moldovan Political History'),('Moldovan Foreign Relations History'),
 ('Chișinău History'),('Moldovan Travel'))
INSERT INTO concept(preferred_label)
SELECT label FROM wanted
WHERE NOT EXISTS (SELECT 1 FROM concept WHERE preferred_label=label);

INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT concept_id,3594,0 FROM concept WHERE preferred_label IN (
 'Moldovan Antiquities','Moldovan Social & Cultural History','Moldovan Ethnography',
 'Moldovan Historical Biographies','Moldovan Historiography','Moldovan Military History',
 'Moldovan Political History','Moldovan Foreign Relations History','Chișinău History');
INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT concept_id,2455,0 FROM concept WHERE preferred_label='Moldovan Travel';

INSERT OR IGNORE INTO lcc_selector(concept_id,selector)
SELECT concept_id,'DK509.3' FROM concept WHERE preferred_label='Moldovan Antiquities';
INSERT OR IGNORE INTO lcc_selector(concept_id,selector)
SELECT concept_id,'DK509.32' FROM concept WHERE preferred_label='Moldovan Social & Cultural History';
INSERT OR IGNORE INTO lcc_range(class_letters,start_value,end_value,concept_id)
SELECT 'DK','509.33','509.36',concept_id FROM concept WHERE preferred_label='Moldovan Ethnography';
INSERT OR IGNORE INTO lcc_range(class_letters,start_value,end_value,concept_id)
SELECT 'DK','509.38','509.45',concept_id FROM concept WHERE preferred_label='Moldovan Historical Biographies';
INSERT OR IGNORE INTO lcc_range(class_letters,start_value,end_value,concept_id)
SELECT 'DK','509.46','509.48',concept_id FROM concept WHERE preferred_label='Moldovan Historiography';
INSERT OR IGNORE INTO lcc_range(class_letters,start_value,end_value,concept_id)
SELECT 'DK','509.6','509.62',concept_id FROM concept WHERE preferred_label='Moldovan Military History';
INSERT OR IGNORE INTO lcc_range(class_letters,start_value,end_value,concept_id)
SELECT 'DK','509.65','509.66',concept_id FROM concept WHERE preferred_label='Moldovan Political History';
INSERT OR IGNORE INTO lcc_range(class_letters,start_value,end_value,concept_id)
SELECT 'DK','509.67','509.69',concept_id FROM concept WHERE preferred_label='Moldovan Foreign Relations History';
DELETE FROM lcc_range WHERE concept_id=3594 AND class_letters='DK'
  AND start_value='509.92' AND end_value='509.939';
INSERT OR IGNORE INTO lcc_range(class_letters,start_value,end_value,concept_id)
SELECT 'DK','509.92','509.939',concept_id FROM concept WHERE preferred_label='Chișinău History';
INSERT OR IGNORE INTO lcc_selector(concept_id,selector)
SELECT concept_id,v FROM concept CROSS JOIN (SELECT 'DK509.22' v UNION ALL SELECT 'DK509.29' UNION ALL SELECT 'DK509.924')
WHERE preferred_label='Moldovan Travel';

-- Luxembourg: remove redundant overlapping umbrella ranges and route each
-- official block to a meaningful leaf. Guidebooks and description/travel are
-- assigned to travel, including the post-1945 schedule.
WITH wanted(label) AS (VALUES
 ('Luxembourg Historical Reference'),('Luxembourg Antiquities'),
 ('Luxembourg Social & Cultural History'),('Luxembourg Ethnography'),
 ('Luxembourg Political & Diplomatic History'),('Early Luxembourg, to 1354'),
 ('Duchy of Luxembourg, 1354–1815'),('Luxembourg City History'),
 ('Luxembourg Travel'))
INSERT INTO concept(preferred_label)
SELECT label FROM wanted
WHERE NOT EXISTS (SELECT 1 FROM concept WHERE preferred_label=label);

INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT concept_id,3578,0 FROM concept WHERE preferred_label IN (
 'Luxembourg Historical Reference','Luxembourg Antiquities',
 'Luxembourg Social & Cultural History','Luxembourg Ethnography',
 'Luxembourg Political & Diplomatic History','Early Luxembourg, to 1354',
 'Duchy of Luxembourg, 1354–1815','Luxembourg City History');
INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT concept_id,2455,0 FROM concept WHERE preferred_label='Luxembourg Travel';

DELETE FROM lcc_range WHERE concept_id=3578 AND class_letters='DH';
INSERT OR IGNORE INTO lcc_range(class_letters,start_value,end_value,concept_id)
SELECT 'DH','901','905.5',concept_id FROM concept WHERE preferred_label='Luxembourg Historical Reference';
INSERT OR IGNORE INTO lcc_selector(concept_id,selector)
SELECT concept_id,'DH907.3' FROM concept WHERE preferred_label='Luxembourg Antiquities';
INSERT OR IGNORE INTO lcc_selector(concept_id,selector)
SELECT concept_id,'DH907.7' FROM concept WHERE preferred_label='Luxembourg Social & Cultural History';
INSERT OR IGNORE INTO lcc_selector(concept_id,selector)
SELECT concept_id,'DH907.9' FROM concept WHERE preferred_label='Luxembourg Ethnography';
INSERT OR IGNORE INTO lcc_range(class_letters,start_value,end_value,concept_id)
SELECT 'DH','908.5','908.6.Z',concept_id FROM concept WHERE preferred_label='Luxembourg Political & Diplomatic History';
INSERT OR IGNORE INTO lcc_selector(concept_id,selector)
SELECT concept_id,'DH909' FROM concept WHERE preferred_label='Early Luxembourg, to 1354';
INSERT OR IGNORE INTO lcc_range(class_letters,start_value,end_value,concept_id)
SELECT 'DH','910','915',concept_id FROM concept WHERE preferred_label='Duchy of Luxembourg, 1354–1815';
INSERT OR IGNORE INTO lcc_range(class_letters,start_value,end_value,concept_id)
SELECT 'DH','925.L8','925.L899',concept_id FROM concept WHERE preferred_label='Luxembourg City History';
INSERT OR IGNORE INTO lcc_selector(concept_id,selector)
SELECT concept_id,'DH903.2' FROM concept WHERE preferred_label='Luxembourg Travel';
INSERT OR IGNORE INTO lcc_range(class_letters,start_value,end_value,concept_id)
SELECT 'DH','906','907',concept_id FROM concept WHERE preferred_label='Luxembourg Travel';

COMMIT;

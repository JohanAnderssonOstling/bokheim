BEGIN;

-- Iceland: separate the three explicit cultural schedules and Reykjavík.
UPDATE concept SET preferred_label='Icelandic Social & Cultural History'
WHERE concept_id=14017;
DELETE FROM lcc_range WHERE concept_id=14017;
INSERT INTO lcc_range(class_letters,start_value,end_value,concept_id)
VALUES ('DL','326','328',14017);

INSERT INTO concept(preferred_label) SELECT 'Icelandic Antiquities'
WHERE NOT EXISTS (SELECT 1 FROM concept WHERE preferred_label='Icelandic Antiquities');
INSERT INTO concept(preferred_label) SELECT 'Icelandic Ethnography'
WHERE NOT EXISTS (SELECT 1 FROM concept WHERE preferred_label='Icelandic Ethnography');
INSERT INTO concept(preferred_label) SELECT 'Reykjavík History'
WHERE NOT EXISTS (SELECT 1 FROM concept WHERE preferred_label='Reykjavík History');

INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT concept_id,3582,0 FROM concept
WHERE preferred_label IN ('Icelandic Antiquities','Icelandic Ethnography','Reykjavík History');
INSERT OR IGNORE INTO lcc_selector(concept_id,selector)
SELECT concept_id,'DL321' FROM concept WHERE preferred_label='Icelandic Antiquities';
INSERT OR IGNORE INTO lcc_range(class_letters,start_value,end_value,concept_id)
SELECT 'DL','331','334.5',concept_id FROM concept WHERE preferred_label='Icelandic Ethnography';
INSERT OR IGNORE INTO lcc_range(class_letters,start_value,end_value,concept_id)
SELECT 'DL','398.R5','398.R59',concept_id FROM concept WHERE preferred_label='Reykjavík History';

-- Belarus: the official schedule supplies independent thematic ranges and a
-- substantial Minsk range; none needs an artificial grouping node.
WITH wanted(label) AS (VALUES
 ('Belarusian Antiquities'),('Belarusian Social & Cultural History'),
 ('Belarusian Ethnography'),('Belarusian Historical Biographies'),
 ('Belarusian Historiography'),('Belarusian Military History'),
 ('Belarusian Political History'),('Belarusian Foreign Relations History'),
 ('Minsk History'))
INSERT INTO concept(preferred_label)
SELECT label FROM wanted
WHERE NOT EXISTS (SELECT 1 FROM concept WHERE preferred_label=label);

INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT concept_id,3587,0 FROM concept WHERE preferred_label IN (
 'Belarusian Antiquities','Belarusian Social & Cultural History',
 'Belarusian Ethnography','Belarusian Historical Biographies',
 'Belarusian Historiography','Belarusian Military History',
 'Belarusian Political History','Belarusian Foreign Relations History',
 'Minsk History');

INSERT OR IGNORE INTO lcc_selector(concept_id,selector)
SELECT concept_id,'DK507.3' FROM concept WHERE preferred_label='Belarusian Antiquities';
INSERT OR IGNORE INTO lcc_selector(concept_id,selector)
SELECT concept_id,'DK507.32' FROM concept WHERE preferred_label='Belarusian Social & Cultural History';
INSERT OR IGNORE INTO lcc_range(class_letters,start_value,end_value,concept_id)
SELECT 'DK','507.33','507.36',concept_id FROM concept WHERE preferred_label='Belarusian Ethnography';
INSERT OR IGNORE INTO lcc_range(class_letters,start_value,end_value,concept_id)
SELECT 'DK','507.38','507.45',concept_id FROM concept WHERE preferred_label='Belarusian Historical Biographies';
INSERT OR IGNORE INTO lcc_range(class_letters,start_value,end_value,concept_id)
SELECT 'DK','507.46','507.48',concept_id FROM concept WHERE preferred_label='Belarusian Historiography';
INSERT OR IGNORE INTO lcc_range(class_letters,start_value,end_value,concept_id)
SELECT 'DK','507.6','507.62',concept_id FROM concept WHERE preferred_label='Belarusian Military History';
INSERT OR IGNORE INTO lcc_range(class_letters,start_value,end_value,concept_id)
SELECT 'DK','507.65','507.66',concept_id FROM concept WHERE preferred_label='Belarusian Political History';
INSERT OR IGNORE INTO lcc_range(class_letters,start_value,end_value,concept_id)
SELECT 'DK','507.67','507.69',concept_id FROM concept WHERE preferred_label='Belarusian Foreign Relations History';
DELETE FROM lcc_range WHERE concept_id=3587 AND class_letters='DK'
  AND start_value='507.92' AND end_value='507.939';
INSERT OR IGNORE INTO lcc_range(class_letters,start_value,end_value,concept_id)
SELECT 'DK','507.92','507.939',concept_id FROM concept WHERE preferred_label='Minsk History';

-- Malta: extract the high-use thematic schedules and the two named places.
WITH wanted(label) AS (VALUES
 ('Maltese Antiquities'),('Maltese Social & Cultural History'),
 ('Maltese Ethnography'),('Maltese Historical Biographies'),
 ('Maltese Historiography'),('Maltese Military History'),
 ('Maltese Naval History'),('Maltese Political History'),
 ('Maltese Foreign Relations History'),('Gozo History'),('Valletta History'))
INSERT INTO concept(preferred_label)
SELECT label FROM wanted
WHERE NOT EXISTS (SELECT 1 FROM concept WHERE preferred_label=label);

INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT concept_id,3574,0 FROM concept WHERE preferred_label IN (
 'Maltese Antiquities','Maltese Social & Cultural History','Maltese Ethnography',
 'Maltese Historical Biographies','Maltese Historiography',
 'Maltese Military History','Maltese Naval History','Maltese Political History',
 'Maltese Foreign Relations History','Gozo History','Valletta History');

INSERT OR IGNORE INTO lcc_selector(concept_id,selector)
SELECT concept_id,'DG989.5' FROM concept WHERE preferred_label='Maltese Antiquities';
INSERT OR IGNORE INTO lcc_selector(concept_id,selector)
SELECT concept_id,'DG989.7' FROM concept WHERE preferred_label='Maltese Social & Cultural History';
INSERT OR IGNORE INTO lcc_range(class_letters,start_value,end_value,concept_id)
SELECT 'DG','989.74','989.75.Z',concept_id FROM concept WHERE preferred_label='Maltese Ethnography';
INSERT OR IGNORE INTO lcc_selector(concept_id,selector)
SELECT concept_id,'DG989.83' FROM concept WHERE preferred_label='Maltese Historical Biographies';
INSERT OR IGNORE INTO lcc_selector(concept_id,selector)
SELECT concept_id,'DG989.85' FROM concept WHERE preferred_label='Maltese Historiography';
INSERT OR IGNORE INTO lcc_selector(concept_id,selector)
SELECT concept_id,'DG990.5' FROM concept WHERE preferred_label='Maltese Military History';
INSERT OR IGNORE INTO lcc_selector(concept_id,selector)
SELECT concept_id,'DG990.7' FROM concept WHERE preferred_label='Maltese Naval History';
INSERT OR IGNORE INTO lcc_selector(concept_id,selector)
SELECT concept_id,'DG991' FROM concept WHERE preferred_label='Maltese Political History';
INSERT OR IGNORE INTO lcc_range(class_letters,start_value,end_value,concept_id)
SELECT 'DG','991.5','991.6.Z',concept_id FROM concept WHERE preferred_label='Maltese Foreign Relations History';
INSERT OR IGNORE INTO lcc_range(class_letters,start_value,end_value,concept_id)
SELECT 'DG','999.G59','999.G599',concept_id FROM concept WHERE preferred_label='Gozo History';
INSERT OR IGNORE INTO lcc_range(class_letters,start_value,end_value,concept_id)
SELECT 'DG','999.V3','999.V399',concept_id FROM concept WHERE preferred_label='Valletta History';

COMMIT;

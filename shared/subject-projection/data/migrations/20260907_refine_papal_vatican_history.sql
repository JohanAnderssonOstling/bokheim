BEGIN;

WITH wanted(label) AS (VALUES
 ('Papal States & Vatican Historical Reference'),('Papal States Antiquities'),
 ('Papal States General History'),('Papal States Military & Naval History'),
 ('Papal States to 1499'),('Papal States, 1499–1700'),
 ('Papal States, 1700–1870'),('Holy See since 1870'),
 ('Vatican City since 1929'))
INSERT INTO concept(preferred_label)
SELECT label FROM wanted WHERE NOT EXISTS
 (SELECT 1 FROM concept WHERE preferred_label=label);

INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT concept_id,10562,0 FROM concept WHERE preferred_label IN (
 'Papal States & Vatican Historical Reference','Papal States Antiquities',
 'Papal States General History','Papal States Military & Naval History',
 'Papal States to 1499','Papal States, 1499–1700','Papal States, 1700–1870',
 'Holy See since 1870','Vatican City since 1929');

DELETE FROM lcc_range WHERE concept_id=10562 AND class_letters='DG';
INSERT INTO lcc_range SELECT 'DG','791','792.5',concept_id FROM concept WHERE preferred_label='Papal States & Vatican Historical Reference';
INSERT INTO lcc_selector SELECT concept_id,'DG795' FROM concept WHERE preferred_label='Papal States Antiquities';
INSERT INTO lcc_range SELECT 'DG','796','796.3',concept_id FROM concept WHERE preferred_label='Papal States General History';
INSERT INTO lcc_selector SELECT concept_id,'DG796.5' FROM concept WHERE preferred_label='Papal States Military & Naval History';
INSERT INTO lcc_range SELECT 'DG','797','797.6',concept_id FROM concept WHERE preferred_label='Papal States to 1499';
INSERT INTO lcc_range SELECT 'DG','797.7','797.93',concept_id FROM concept WHERE preferred_label='Papal States, 1499–1700';
INSERT INTO lcc_range SELECT 'DG','798','798.7',concept_id FROM concept WHERE preferred_label='Papal States, 1700–1870';
INSERT INTO lcc_selector SELECT concept_id,'DG799' FROM concept WHERE preferred_label='Holy See since 1870';
INSERT INTO lcc_selector SELECT concept_id,'DG800' FROM concept WHERE preferred_label='Vatican City since 1929';

-- DG794 is explicitly description and travel.
INSERT OR IGNORE INTO lcc_selector(concept_id,selector) VALUES (2464,'DG794');

COMMIT;

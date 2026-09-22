BEGIN;

WITH wanted(label) AS (VALUES
 ('Cypriot History'),('Cypriot Historical Reference'),('Cypriot Antiquities'),
 ('Cypriot Social & Cultural History'),('Cypriot Ethnography'),
 ('Cypriot Political & Diplomatic History'),('Cyprus to 1571'),
 ('Ottoman Cyprus, 1571–1878'),('British Cyprus, 1878–1960'),
 ('Republic of Cyprus, 1960–Present'),('Northern Cyprus History'),
 ('Nicosia History'),('Larnaca History'),('Paphos History'))
INSERT INTO concept(preferred_label)
SELECT label FROM wanted
WHERE NOT EXISTS (SELECT 1 FROM concept WHERE preferred_label=label);

INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT concept_id,5541,0 FROM concept WHERE preferred_label='Cypriot History';

INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT concept_id,(SELECT concept_id FROM concept WHERE preferred_label='Cypriot History'),0
FROM concept WHERE preferred_label IN (
 'Cypriot Historical Reference','Cypriot Antiquities',
 'Cypriot Social & Cultural History','Cypriot Ethnography',
 'Cypriot Political & Diplomatic History','Cyprus to 1571',
 'Ottoman Cyprus, 1571–1878','British Cyprus, 1878–1960',
 'Republic of Cyprus, 1960–Present','Northern Cyprus History',
 'Nicosia History','Larnaca History','Paphos History');

INSERT OR IGNORE INTO lcc_range(class_letters,start_value,end_value,concept_id)
SELECT 'DS','54.A2','54.A35',concept_id FROM concept WHERE preferred_label='Cypriot Historical Reference';
INSERT OR IGNORE INTO lcc_selector(concept_id,selector)
SELECT concept_id,'DS54.3' FROM concept WHERE preferred_label='Cypriot Antiquities';
INSERT OR IGNORE INTO lcc_selector(concept_id,selector)
SELECT concept_id,'DS54.35' FROM concept WHERE preferred_label='Cypriot Social & Cultural History';
INSERT OR IGNORE INTO lcc_range(class_letters,start_value,end_value,concept_id)
SELECT 'DS','54.4','54.44',concept_id FROM concept WHERE preferred_label='Cypriot Ethnography';
INSERT OR IGNORE INTO lcc_range(class_letters,start_value,end_value,concept_id)
SELECT 'DS','54.55','54.56.Z',concept_id FROM concept WHERE preferred_label='Cypriot Political & Diplomatic History';
INSERT OR IGNORE INTO lcc_range(class_letters,start_value,end_value,concept_id)
SELECT 'DS','54.6','54.66',concept_id FROM concept WHERE preferred_label='Cyprus to 1571';
INSERT OR IGNORE INTO lcc_selector(concept_id,selector)
SELECT concept_id,'DS54.7' FROM concept WHERE preferred_label='Ottoman Cyprus, 1571–1878';
INSERT OR IGNORE INTO lcc_range(class_letters,start_value,end_value,concept_id)
SELECT 'DS','54.8','54.86',concept_id FROM concept WHERE preferred_label='British Cyprus, 1878–1960';
INSERT OR IGNORE INTO lcc_range(class_letters,start_value,end_value,concept_id)
SELECT 'DS','54.9','54.93.Z',concept_id FROM concept WHERE preferred_label='Republic of Cyprus, 1960–Present';
INSERT OR IGNORE INTO lcc_range(class_letters,start_value,end_value,concept_id)
SELECT 'DS','54.95.N67','54.95.N679',concept_id FROM concept WHERE preferred_label='Northern Cyprus History';
INSERT OR IGNORE INTO lcc_range(class_letters,start_value,end_value,concept_id)
SELECT 'DS','54.95.N53','54.95.N539',concept_id FROM concept WHERE preferred_label='Nicosia History';
INSERT OR IGNORE INTO lcc_range(class_letters,start_value,end_value,concept_id)
SELECT 'DS','54.95.L37','54.95.L379',concept_id FROM concept WHERE preferred_label='Larnaca History';
INSERT OR IGNORE INTO lcc_range(class_letters,start_value,end_value,concept_id)
SELECT 'DS','54.95.N47','54.95.N479',concept_id FROM concept WHERE preferred_label='Paphos History';
INSERT OR IGNORE INTO lcc_range(class_letters,start_value,end_value,concept_id)
SELECT 'DS','54.95.O43','54.95.O439',concept_id FROM concept WHERE preferred_label='Paphos History';

-- Remaining named localities stay in a mutually exclusive broad locality leaf.
INSERT INTO concept(preferred_label)
SELECT 'Cypriot Local History'
WHERE NOT EXISTS (SELECT 1 FROM concept WHERE preferred_label='Cypriot Local History');
INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT concept_id,(SELECT concept_id FROM concept WHERE preferred_label='Cypriot History'),0
FROM concept WHERE preferred_label='Cypriot Local History';
INSERT OR IGNORE INTO lcc_range(class_letters,start_value,end_value,concept_id)
SELECT 'DS','54.95.A','54.95.Z',concept_id FROM concept WHERE preferred_label='Cypriot Local History';

-- Exact/narrow locality ranges take precedence over the locality umbrella.
INSERT OR IGNORE INTO lcc_selector(concept_id,selector)
SELECT 2458,'DS54.A288';
INSERT OR IGNORE INTO lcc_range(class_letters,start_value,end_value,concept_id)
VALUES ('DS','54.A4','54.Z',2458);

COMMIT;

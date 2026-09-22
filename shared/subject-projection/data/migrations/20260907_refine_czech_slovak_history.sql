BEGIN;

INSERT INTO concept(preferred_label) VALUES
 ('Czechoslovak Biographies'),('Czechoslovak Historiography'),
 ('Czechoslovak Political History'),('Czechoslovak Foreign Relations History'),
 ('Bratislava History'),('Košice History'),('Tatra Mountains History');

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT concept_id,3538,0 FROM concept WHERE preferred_label IN (
 'Czechoslovak Biographies','Czechoslovak Historiography',
 'Czechoslovak Political History','Czechoslovak Foreign Relations History'
);
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT concept_id,3540,0 FROM concept WHERE preferred_label IN (
 'Bratislava History','Košice History','Tatra Mountains History'
);

INSERT INTO lcc_range(class_letters,start_value,end_value,concept_id)
SELECT 'DB','2045','2051.Z',concept_id FROM concept WHERE preferred_label='Czechoslovak Biographies';
INSERT INTO lcc_range(class_letters,start_value,end_value,concept_id)
SELECT 'DB','2055','2057.Z',concept_id FROM concept WHERE preferred_label='Czechoslovak Historiography';
INSERT INTO lcc_range(class_letters,start_value,end_value,concept_id)
SELECT 'DB','2074','2075.Z',concept_id FROM concept WHERE preferred_label='Czechoslovak Political History';
INSERT INTO lcc_range(class_letters,start_value,end_value,concept_id)
SELECT 'DB','2076','2078.Z',concept_id FROM concept WHERE preferred_label='Czechoslovak Foreign Relations History';
INSERT INTO lcc_range(class_letters,start_value,end_value,concept_id)
SELECT 'DB','3100','3149.Z',concept_id FROM concept WHERE preferred_label='Bratislava History';

INSERT INTO lcc_selector(concept_id,selector)
SELECT concept_id,'DB3150.K67' FROM concept WHERE preferred_label='Košice History';
INSERT INTO lcc_selector(concept_id,selector)
SELECT concept_id,'DB3000.T37' FROM concept WHERE preferred_label='Tatra Mountains History';

INSERT OR IGNORE INTO lcc_selector(concept_id,selector) VALUES
 (13996,'DB2010'),(13996,'DB2607'),(14004,'DB2710'),(14004,'DB3107');

DELETE FROM lcc_selector
WHERE selector='DB2710' AND concept_id<>14004;

COMMIT;

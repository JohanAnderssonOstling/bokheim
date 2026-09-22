BEGIN;

INSERT INTO concept(preferred_label) VALUES
 ('Estonian Antiquities'),('Estonian Social & Cultural History'),('Estonian Ethnography'),
 ('Estonians Abroad'),('Estonian Biographies'),('Estonian Historiography'),
 ('Estonian Political History'),('Estonian Foreign Relations History'),('Tartu History'),
 ('Latvian Antiquities'),('Latvian Social & Cultural History'),('Latvian Ethnography'),
 ('Latvians Abroad'),('Latvian Biographies'),('Latvian Historiography'),
 ('Latvian Political History'),('Latvian Foreign Relations History'),('Liepāja History'),
 ('Lithuanian Antiquities'),('Lithuanian Social & Cultural History'),('Lithuanian Ethnography'),
 ('Lithuanians Abroad'),('Lithuanian Biographies'),('Lithuanian Historiography'),
 ('Lithuanian Political History'),('Lithuanian Foreign Relations History'),
 ('Kaunas History'),('Klaipėda History');

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT concept_id,11216,0 FROM concept WHERE preferred_label IN (
 'Estonian Antiquities','Estonian Social & Cultural History','Estonian Ethnography',
 'Estonians Abroad','Estonian Biographies','Estonian Historiography',
 'Estonian Political History','Estonian Foreign Relations History','Tartu History');
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT concept_id,11217,0 FROM concept WHERE preferred_label IN (
 'Latvian Antiquities','Latvian Social & Cultural History','Latvian Ethnography',
 'Latvians Abroad','Latvian Biographies','Latvian Historiography',
 'Latvian Political History','Latvian Foreign Relations History','Liepāja History');
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT concept_id,11221,0 FROM concept WHERE preferred_label IN (
 'Lithuanian Antiquities','Lithuanian Social & Cultural History','Lithuanian Ethnography',
 'Lithuanians Abroad','Lithuanian Biographies','Lithuanian Historiography',
 'Lithuanian Political History','Lithuanian Foreign Relations History',
 'Kaunas History','Klaipėda History');

UPDATE concept_parent SET parent_concept_id=(SELECT concept_id FROM concept WHERE preferred_label='Estonian Ethnography')
WHERE concept_id=(SELECT concept_id FROM concept WHERE preferred_label='Estonians Abroad') AND parent_concept_id=11216;
UPDATE concept_parent SET parent_concept_id=(SELECT concept_id FROM concept WHERE preferred_label='Latvian Ethnography')
WHERE concept_id=(SELECT concept_id FROM concept WHERE preferred_label='Latvians Abroad') AND parent_concept_id=11217;
UPDATE concept_parent SET parent_concept_id=(SELECT concept_id FROM concept WHERE preferred_label='Lithuanian Ethnography')
WHERE concept_id=(SELECT concept_id FROM concept WHERE preferred_label='Lithuanians Abroad') AND parent_concept_id=11221;

CREATE TEMP TABLE baltic_range(start_value TEXT,end_value TEXT,label TEXT);
INSERT INTO baltic_range VALUES
 ('503.33','503.36.Z','Estonian Ethnography'),('503.38','503.45.Z','Estonian Biographies'),
 ('503.46','503.48.Z','Estonian Historiography'),('503.65','503.66.Z','Estonian Political History'),
 ('503.67','503.69.Z','Estonian Foreign Relations History'),
 ('504.33','504.36.Z','Latvian Ethnography'),('504.38','504.45.Z','Latvian Biographies'),
 ('504.46','504.48.Z','Latvian Historiography'),('504.65','504.66.Z','Latvian Political History'),
 ('504.67','504.69.Z','Latvian Foreign Relations History'),
 ('505.33','505.36.Z','Lithuanian Ethnography'),('505.38','505.45.Z','Lithuanian Biographies'),
 ('505.46','505.48.Z','Lithuanian Historiography'),('505.65','505.66.Z','Lithuanian Political History'),
 ('505.67','505.69.Z','Lithuanian Foreign Relations History');
INSERT INTO lcc_range(class_letters,start_value,end_value,concept_id)
SELECT 'DK',m.start_value,m.end_value,c.concept_id
FROM baltic_range m JOIN concept c ON c.preferred_label=m.label;

CREATE TEMP TABLE baltic_selector(selector TEXT,label TEXT);
INSERT INTO baltic_selector VALUES
 ('DK503.3','Estonian Antiquities'),('DK503.32','Estonian Social & Cultural History'),
 ('DK503.36','Estonians Abroad'),('DK503.95.T38','Tartu History'),
 ('DK504.3','Latvian Antiquities'),('DK504.32','Latvian Social & Cultural History'),
 ('DK504.36','Latvians Abroad'),('DK504.95.L48','Liepāja History'),
 ('DK505.3','Lithuanian Antiquities'),('DK505.32','Lithuanian Social & Cultural History'),
 ('DK505.36','Lithuanians Abroad'),('DK505.95.K125','Kaunas History'),
 ('DK505.95.K57','Klaipėda History');
INSERT INTO lcc_selector(concept_id,selector)
SELECT c.concept_id,m.selector FROM baltic_selector m JOIN concept c ON c.preferred_label=m.label;

INSERT OR IGNORE INTO lcc_selector(concept_id,selector) VALUES
 (13957,'DK503.22'),(13957,'DK503.924'),
 (13958,'DK504.22'),(13958,'DK504.924'),
 (13959,'DK505.22'),(13959,'DK505.924');

DROP TABLE baltic_range;
DROP TABLE baltic_selector;
COMMIT;

BEGIN;

INSERT INTO concept(preferred_label) VALUES
 ('Danish Antiquities'),('Danish Social & Cultural History'),('Danish Ethnography'),
 ('Danish Biographies'),('Danish Historiography'),('Danish Political History'),
 ('Danish Foreign Relations History'),('Christian X, 1912–1947'),
 ('Frederik IX, 1947–1972'),('Margrethe II, 1972–2024'),
 ('Copenhagen History'),('Odense History'),('Faroe Islands History'),
 ('Jutland History'),('South Jutland History'),
 ('Norwegian Antiquities'),('Norwegian Social & Cultural History'),
 ('Norwegian Ethnography'),('Norwegian Biographies'),('Norwegian Historiography'),
 ('Norwegian Political & Diplomatic History'),('Haakon VII, 1905–1957'),
 ('Olav V, 1957–1991'),('Harald V, 1991–Present'),
 ('Oslo History'),('Bergen History'),('Stavanger History'),
 ('Finnmark History'),('Rogaland History');

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT concept_id,3580,0 FROM concept WHERE preferred_label IN (
 'Danish Antiquities','Danish Social & Cultural History','Danish Ethnography',
 'Danish Biographies','Danish Historiography','Danish Political History',
 'Danish Foreign Relations History','Copenhagen History','Odense History',
 'Faroe Islands History','Jutland History','South Jutland History'
);
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT concept_id,11513,0 FROM concept WHERE preferred_label IN (
 'Christian X, 1912–1947','Frederik IX, 1947–1972','Margrethe II, 1972–2024'
);
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT concept_id,3583,0 FROM concept WHERE preferred_label IN (
 'Norwegian Antiquities','Norwegian Social & Cultural History','Norwegian Ethnography',
 'Norwegian Biographies','Norwegian Historiography','Norwegian Political & Diplomatic History',
 'Oslo History','Bergen History','Stavanger History','Finnmark History','Rogaland History'
);
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT concept_id,11538,0 FROM concept WHERE preferred_label IN (
 'Haakon VII, 1905–1957','Olav V, 1957–1991','Harald V, 1991–Present'
);
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal) VALUES (51397,3583,1);

CREATE TEMP TABLE dn_range(class_letters TEXT,start_value TEXT,end_value TEXT,label TEXT);
INSERT INTO dn_range VALUES
 ('DL','141','142.Z','Danish Ethnography'),('DL','144','144.7.Z','Danish Biographies'),
 ('DL','146','146.7.Z','Danish Historiography'),('DL','159','159.5.Z','Danish Foreign Relations History'),
 ('DL','255','257.Z','Christian X, 1912–1947'),('DL','258','260.Z','Frederik IX, 1947–1972'),
 ('DL','261','263.3.Z','Margrethe II, 1972–2024'),('DL','276','290.9.Z','Copenhagen History'),
 ('DL','441','442.Z','Norwegian Ethnography'),('DL','444','444.7.Z','Norwegian Biographies'),
 ('DL','445','445.7.Z','Norwegian Historiography'),('DL','458','459.Z','Norwegian Political & Diplomatic History'),
 ('DL','530','533.Z','Haakon VII, 1905–1957'),('DL','534','535.Z','Olav V, 1957–1991'),
 ('DL','536','537.Z','Harald V, 1991–Present'),('DL','581','595.9.Z','Oslo History');
INSERT INTO lcc_range(class_letters,start_value,end_value,concept_id)
SELECT m.class_letters,m.start_value,m.end_value,c.concept_id
FROM dn_range m JOIN concept c ON c.preferred_label=m.label;

CREATE TEMP TABLE dn_selector(selector TEXT,label TEXT);
INSERT INTO dn_selector VALUES
 ('DL121','Danish Antiquities'),('DL131','Danish Social & Cultural History'),
 ('DL158','Danish Political History'),('DL291.O3','Odense History'),
 ('DL271.F2','Faroe Islands History'),('DL271.J8','Jutland History'),
 ('DL271.S6','South Jutland History'),('DL421','Norwegian Antiquities'),
 ('DL431','Norwegian Social & Cultural History'),('DL596.B4','Bergen History'),
 ('DL596.S8','Stavanger History'),('DL576.F5','Finnmark History'),
 ('DL576.R65','Rogaland History'),('DL442.L3','Sámi History & Culture'),
 ('DL442.S16','Sámi History & Culture');
INSERT INTO lcc_selector(concept_id,selector)
SELECT c.concept_id,m.selector FROM dn_selector m JOIN concept c ON c.preferred_label=m.label;

INSERT OR IGNORE INTO lcc_selector(concept_id,selector) VALUES (2466,'DL107'),(2469,'DL407');

DROP TABLE dn_range;
DROP TABLE dn_selector;
COMMIT;

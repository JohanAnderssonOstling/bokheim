BEGIN;

INSERT INTO concept(preferred_label) VALUES
 ('Belgian Antiquities'),('Belgian Social & Cultural History'),('Belgian Ethnography'),
 ('Walloons'),('Belgian Biographies'),('Belgian Political History'),
 ('Albert I, 1909–1934'),('Leopold III, 1934–1951'),('Baudouin, 1951–1993'),
 ('Albert II, 1993–2013'),('Philippe, 2013–Present'),
 ('Flanders History'),('Wallonia History'),('Antwerp History'),('Bruges History'),
 ('Finnish Antiquities'),('Finnish Social & Cultural History'),('Finnish Ethnography'),
 ('Swedes in Finland'),('Finnish Historiography'),('Finnish Political History'),
 ('Finnish Foreign Relations History'),('Finnish Revolution, 1917–1918'),
 ('Interwar Finland, 1918–1939'),('Finland in World War II, 1939–1945'),
 ('Postwar Finland, 1945–1981'),('Finland since 1981'),
 ('Åland History'),('Finnish Lapland History'),('Turku History');

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT concept_id,3577,0 FROM concept WHERE preferred_label IN (
 'Belgian Antiquities','Belgian Social & Cultural History','Belgian Ethnography',
 'Belgian Biographies','Belgian Political History','Flanders History','Wallonia History',
 'Antwerp History','Bruges History'
);
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT concept_id,(SELECT concept_id FROM concept WHERE preferred_label='Belgian Ethnography'),0
FROM concept WHERE preferred_label='Walloons';
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT concept_id,13925,0 FROM concept WHERE preferred_label IN (
 'Albert I, 1909–1934','Leopold III, 1934–1951','Baudouin, 1951–1993',
 'Albert II, 1993–2013'
);
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT concept_id,13926,0 FROM concept WHERE preferred_label='Philippe, 2013–Present';

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT concept_id,3581,0 FROM concept WHERE preferred_label IN (
 'Finnish Antiquities','Finnish Social & Cultural History','Finnish Ethnography',
 'Finnish Historiography','Finnish Political History','Finnish Foreign Relations History',
 'Åland History','Finnish Lapland History','Turku History'
);
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT concept_id,(SELECT concept_id FROM concept WHERE preferred_label='Finnish Ethnography'),0
FROM concept WHERE preferred_label='Swedes in Finland';
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT concept_id,11518,0 FROM concept WHERE preferred_label IN (
 'Finnish Revolution, 1917–1918','Interwar Finland, 1918–1939',
 'Finland in World War II, 1939–1945','Postwar Finland, 1945–1981','Finland since 1981'
);

CREATE TEMP TABLE bf_range(class_letters TEXT,start_value TEXT,end_value TEXT,label TEXT);
INSERT INTO bf_range VALUES
 ('DH','491','492.Z','Belgian Ethnography'),('DH','511','519.Z','Belgian Biographies'),
 ('DH','566','569.Z','Belgian Political History'),('DH','681','685.Z','Albert I, 1909–1934'),
 ('DH','687','689.Z','Leopold III, 1934–1951'),('DH','690','692.Z','Baudouin, 1951–1993'),
 ('DH','693','694.Z','Albert II, 1993–2013'),('DH','696','697.Z','Philippe, 2013–Present'),
 ('DH','801.F4','801.F49.Z','Flanders History'),('DH','801.W25','801.W25.Z','Wallonia History'),
 ('DL','1018','1022.Z','Finnish Ethnography'),('DL','1025','1026.5.Z','Finnish Historiography'),
 ('DL','1043','1044.Z','Finnish Political History'),('DL','1045','1048.Z','Finnish Foreign Relations History'),
 ('DL','1070','1075.Z','Finnish Revolution, 1917–1918'),
 ('DL','1082','1088.5.Z','Interwar Finland, 1918–1939'),
 ('DL','1090','1105.Z','Finland in World War II, 1939–1945'),
 ('DL','1122','1135.5.Z','Postwar Finland, 1945–1981'),
 ('DL','1140','1141.6.Z','Finland since 1981');
INSERT INTO lcc_range(class_letters,start_value,end_value,concept_id)
SELECT m.class_letters,m.start_value,m.end_value,c.concept_id
FROM bf_range m JOIN concept c ON c.preferred_label=m.label;

CREATE TEMP TABLE bf_selector(selector TEXT,label TEXT);
INSERT INTO bf_selector VALUES
 ('DH451','Belgian Antiquities'),('DH471','Belgian Social & Cultural History'),
 ('DH492.W3','Walloons'),('DH811.A58','Antwerp History'),('DH811.B78','Bruges History'),
 ('DL1016','Finnish Antiquities'),('DL1017','Finnish Social & Cultural History'),
 ('DL1020.S93','Swedes in Finland'),('DL1170.A4','Åland History'),
 ('DL1170.L36','Finnish Lapland History'),('DL1180.T87','Turku History');
INSERT INTO lcc_selector(concept_id,selector)
SELECT c.concept_id,m.selector FROM bf_selector m JOIN concept c ON c.preferred_label=m.label;

INSERT OR IGNORE INTO lcc_selector(concept_id,selector) VALUES
 (13919,'DH416'),(13919,'DH804'),(2467,'DL1010'),(2467,'DL1175.25');

DROP TABLE bf_range;
DROP TABLE bf_selector;
COMMIT;

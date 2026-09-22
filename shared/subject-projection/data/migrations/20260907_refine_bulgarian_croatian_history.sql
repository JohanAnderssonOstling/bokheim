BEGIN;

INSERT INTO concept(preferred_label) VALUES
 ('Bulgarian Antiquities'),('Bulgarian Social & Cultural History'),
 ('Bulgarian Ethnography'),('Turks in Bulgaria'),('Bulgarian Biographies'),
 ('Bulgarian Historiography'),('Bulgarian Political & Diplomatic History'),
 ('Sofia History'),('Plovdiv History'),('Veliko Tarnovo History'),
 ('Croatian Antiquities'),('Croatian Social & Cultural History'),
 ('Croatian Ethnography'),('Serbs in Croatia'),('Croatian Biographies'),
 ('Croatian Historiography'),('Croatian Political History'),
 ('Croatian Foreign Relations History'),('Early Croatian Kingdom, to 1102'),
 ('Croatia under Arpad & Angevin Rule, 1102–1527'),
 ('Habsburg Croatia, 1527–1918'),('Croatia within Yugoslavia, 1918–1945'),
 ('Socialist Croatia, 1945–1990'),('Croatia since 1990'),
 ('Croatian Political History since 1990'),('Dalmatian History'),
 ('Slavonian History'),('Zagreb History'),('Dubrovnik History');

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT concept_id,3545,0 FROM concept WHERE preferred_label IN (
 'Bulgarian Antiquities','Bulgarian Social & Cultural History','Bulgarian Ethnography',
 'Bulgarian Biographies','Bulgarian Historiography','Bulgarian Political & Diplomatic History',
 'Sofia History','Plovdiv History','Veliko Tarnovo History'
);
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT concept_id,(SELECT concept_id FROM concept WHERE preferred_label='Bulgarian Ethnography'),0
FROM concept WHERE preferred_label='Turks in Bulgaria';

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT concept_id,3546,0 FROM concept WHERE preferred_label IN (
 'Croatian Antiquities','Croatian Social & Cultural History','Croatian Ethnography',
 'Croatian Biographies','Croatian Historiography','Croatian Political History',
 'Croatian Foreign Relations History','Early Croatian Kingdom, to 1102',
 'Croatia under Arpad & Angevin Rule, 1102–1527','Habsburg Croatia, 1527–1918',
 'Croatia within Yugoslavia, 1918–1945','Socialist Croatia, 1945–1990',
 'Croatia since 1990','Dalmatian History','Slavonian History','Zagreb History','Dubrovnik History'
);
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT concept_id,(SELECT concept_id FROM concept WHERE preferred_label='Croatian Ethnography'),0
FROM concept WHERE preferred_label='Serbs in Croatia';
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT concept_id,(SELECT concept_id FROM concept WHERE preferred_label='Croatia since 1990'),0
FROM concept WHERE preferred_label='Croatian Political History since 1990';

CREATE TEMP TABLE bc_range(class_letters TEXT,start_value TEXT,end_value TEXT,label TEXT);
INSERT INTO bc_range VALUES
 ('DR','64','64.2.Z','Bulgarian Ethnography'),('DR','66','66.6.Z','Bulgarian Biographies'),
 ('DR','66.7','66.9.Z','Bulgarian Historiography'),('DR','72','73.Z','Bulgarian Political & Diplomatic History'),
 ('DR','97','97.9.Z','Sofia History'),
 ('DR','1523','1524.Z','Croatian Ethnography'),('DR','1526','1528.Z','Croatian Biographies'),
 ('DR','1531','1532.5.Z','Croatian Historiography'),('DR','1545','1546.Z','Croatian Foreign Relations History'),
 ('DR','1547','1557.Z','Early Croatian Kingdom, to 1102'),
 ('DR','1559','1566.Z','Croatia under Arpad & Angevin Rule, 1102–1527'),
 ('DR','1567','1582.Z','Habsburg Croatia, 1527–1918'),
 ('DR','1583','1591.Z','Croatia within Yugoslavia, 1918–1945'),
 ('DR','1592','1598.Z','Socialist Croatia, 1945–1990'),
 ('DR','1600','1605.2.Z','Croatia since 1990'),
 ('DR','1620','1630.5.Z','Dalmatian History'),('DR','1633','1636.5.Z','Slavonian History'),
 ('DR','1638','1644.9.Z','Zagreb History');
INSERT INTO lcc_range(class_letters,start_value,end_value,concept_id)
SELECT m.class_letters,m.start_value,m.end_value,c.concept_id
FROM bc_range m JOIN concept c ON c.preferred_label=m.label;

CREATE TEMP TABLE bc_selector(selector TEXT,label TEXT);
INSERT INTO bc_selector VALUES
 ('DR62','Bulgarian Antiquities'),('DR63','Bulgarian Social & Cultural History'),
 ('DR64.2.T87','Turks in Bulgaria'),('DR98.P55','Plovdiv History'),
 ('DR98.T5','Veliko Tarnovo History'),('DR1521','Croatian Antiquities'),
 ('DR1522','Croatian Social & Cultural History'),('DR1524.S47','Serbs in Croatia'),
 ('DR1542','Croatian Political History'),('DR1603','Croatian Political History since 1990'),
 ('DR1645.D8','Dubrovnik History');
INSERT INTO lcc_selector(concept_id,selector)
SELECT c.concept_id,m.selector FROM bc_selector m JOIN concept c ON c.preferred_label=m.label;

INSERT OR IGNORE INTO lcc_selector(concept_id,selector) VALUES
 (13915,'DR54'),(2455,'DR1509'),(2455,'DR1517'),
 (2455,'DR1620.9'),(2455,'DR1633.9');

DROP TABLE bc_range;
DROP TABLE bc_selector;
COMMIT;

BEGIN;

WITH wanted(label) AS (VALUES
 ('Slovenian History'),('Slovenian Historical Reference'),('Slovenian Antiquities'),
 ('Slovenian Social & Cultural History'),('Slovenian Ethnography'),
 ('Slovenian Historical Studies'),('Slovenia to 1456'),
 ('Habsburg Slovenia, 1456–1918'),('Yugoslav Slovenia, 1918–1990'),
 ('Independent Slovenia, 1990–Present'),('Slovenian Local History'),('Slovenian Travel'),
 ('Bosnia and Herzegovina History'),('Bosnian Historical Reference'),('Bosnian Antiquities'),
 ('Bosnian Social & Cultural History'),('Bosnian Ethnography'),('Bosnian Historical Studies'),
 ('Bosnia to 1463'),('Ottoman Bosnia, 1463–1878'),('Austrian Bosnia, 1878–1918'),
 ('Yugoslav Bosnia, 1918–1992'),('Independent Bosnia and Herzegovina, 1992–Present'),
 ('Bosnian Local History'),('Bosnian Travel'),
 ('North Macedonian History'),('North Macedonian Historical Reference'),
 ('North Macedonian Antiquities'),('North Macedonian Social & Cultural History'),
 ('North Macedonian Ethnography'),('North Macedonian Historical Studies'),
 ('North Macedonian Political & Diplomatic History'),('Macedonia to 1389'),
 ('Ottoman Macedonia, 1389–1912'),('Macedonia, 1912–1945'),
 ('Socialist Macedonia, 1945–1992'),('Independent North Macedonia, 1992–Present'),
 ('North Macedonian Local History'),('North Macedonian Travel'))
INSERT INTO concept(preferred_label)
SELECT label FROM wanted WHERE NOT EXISTS
 (SELECT 1 FROM concept WHERE preferred_label=label);

INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT concept_id,3541,0 FROM concept WHERE preferred_label IN
 ('Slovenian History','Bosnia and Herzegovina History','North Macedonian History');

INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT c.concept_id,r.concept_id,0 FROM concept c JOIN concept r
WHERE r.preferred_label='Slovenian History' AND c.preferred_label IN (
 'Slovenian Historical Reference','Slovenian Antiquities','Slovenian Social & Cultural History',
 'Slovenian Ethnography','Slovenian Historical Studies','Slovenia to 1456',
 'Habsburg Slovenia, 1456–1918','Yugoslav Slovenia, 1918–1990',
 'Independent Slovenia, 1990–Present','Slovenian Local History');
INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT c.concept_id,r.concept_id,0 FROM concept c JOIN concept r
WHERE r.preferred_label='Bosnia and Herzegovina History' AND c.preferred_label IN (
 'Bosnian Historical Reference','Bosnian Antiquities','Bosnian Social & Cultural History',
 'Bosnian Ethnography','Bosnian Historical Studies','Bosnia to 1463',
 'Ottoman Bosnia, 1463–1878','Austrian Bosnia, 1878–1918','Yugoslav Bosnia, 1918–1992',
 'Independent Bosnia and Herzegovina, 1992–Present','Bosnian Local History');
INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT c.concept_id,r.concept_id,0 FROM concept c JOIN concept r
WHERE r.preferred_label='North Macedonian History' AND c.preferred_label IN (
 'North Macedonian Historical Reference','North Macedonian Antiquities',
 'North Macedonian Social & Cultural History','North Macedonian Ethnography',
 'North Macedonian Historical Studies','North Macedonian Political & Diplomatic History',
 'Macedonia to 1389','Ottoman Macedonia, 1389–1912','Macedonia, 1912–1945',
 'Socialist Macedonia, 1945–1992','Independent North Macedonia, 1992–Present',
 'North Macedonian Local History');
INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT concept_id,2455,0 FROM concept WHERE preferred_label IN
 ('Slovenian Travel','Bosnian Travel','North Macedonian Travel');

-- Remove the three country schedules from the old Yugoslav umbrella.
DELETE FROM lcc_range WHERE concept_id=10171 AND class_letters='DR' AND
 ((CAST(start_value AS REAL)>=1352 AND CAST(end_value AS REAL)<=1485) OR
  (CAST(start_value AS REAL)>=1652 AND CAST(end_value AS REAL)<=1785) OR
  (CAST(start_value AS REAL)>=2152 AND CAST(end_value AS REAL)<=2285));

-- Slovenia.
INSERT INTO lcc_range SELECT 'DR','1352','1370.999',concept_id FROM concept WHERE preferred_label='Slovenian Historical Reference';
INSERT INTO lcc_selector SELECT concept_id,'DR1371' FROM concept WHERE preferred_label='Slovenian Antiquities';
INSERT INTO lcc_selector SELECT concept_id,'DR1372' FROM concept WHERE preferred_label='Slovenian Social & Cultural History';
INSERT INTO lcc_range SELECT 'DR','1373','1375',concept_id FROM concept WHERE preferred_label='Slovenian Ethnography';
INSERT INTO lcc_range SELECT 'DR','1381','1394.999',concept_id FROM concept WHERE preferred_label='Slovenian Historical Studies';
INSERT INTO lcc_range SELECT 'DR','1395','1404.999',concept_id FROM concept WHERE preferred_label='Slovenia to 1456';
INSERT INTO lcc_range SELECT 'DR','1405','1434.999',concept_id FROM concept WHERE preferred_label='Habsburg Slovenia, 1456–1918';
INSERT INTO lcc_range SELECT 'DR','1435','1450',concept_id FROM concept WHERE preferred_label='Yugoslav Slovenia, 1918–1990';
INSERT INTO lcc_range SELECT 'DR','1452','1457.2',concept_id FROM concept WHERE preferred_label='Independent Slovenia, 1990–Present';
INSERT INTO lcc_range SELECT 'DR','1475','1485',concept_id FROM concept WHERE preferred_label='Slovenian Local History';
INSERT INTO lcc_selector SELECT concept_id,'DR1359' FROM concept WHERE preferred_label='Slovenian Travel';
INSERT INTO lcc_selector SELECT concept_id,'DR1367' FROM concept WHERE preferred_label='Slovenian Travel';

-- Bosnia and Herzegovina.
INSERT INTO lcc_range SELECT 'DR','1652','1670.999',concept_id FROM concept WHERE preferred_label='Bosnian Historical Reference';
INSERT INTO lcc_selector SELECT concept_id,'DR1671' FROM concept WHERE preferred_label='Bosnian Antiquities';
INSERT INTO lcc_selector SELECT concept_id,'DR1672' FROM concept WHERE preferred_label='Bosnian Social & Cultural History';
INSERT INTO lcc_range SELECT 'DR','1673','1674.999',concept_id FROM concept WHERE preferred_label='Bosnian Ethnography';
INSERT INTO lcc_range SELECT 'DR','1681','1694.999',concept_id FROM concept WHERE preferred_label='Bosnian Historical Studies';
INSERT INTO lcc_range SELECT 'DR','1695','1710',concept_id FROM concept WHERE preferred_label='Bosnia to 1463';
INSERT INTO lcc_range SELECT 'DR','1711','1721',concept_id FROM concept WHERE preferred_label='Ottoman Bosnia, 1463–1878';
INSERT INTO lcc_range SELECT 'DR','1722','1732',concept_id FROM concept WHERE preferred_label='Austrian Bosnia, 1878–1918';
INSERT INTO lcc_range SELECT 'DR','1733','1748',concept_id FROM concept WHERE preferred_label='Yugoslav Bosnia, 1918–1992';
INSERT INTO lcc_range SELECT 'DR','1749','1756',concept_id FROM concept WHERE preferred_label='Independent Bosnia and Herzegovina, 1992–Present';
INSERT INTO lcc_range SELECT 'DR','1775','1785',concept_id FROM concept WHERE preferred_label='Bosnian Local History';
INSERT INTO lcc_selector SELECT concept_id,'DR1659' FROM concept WHERE preferred_label='Bosnian Travel';
INSERT INTO lcc_selector SELECT concept_id,'DR1667' FROM concept WHERE preferred_label='Bosnian Travel';

-- North Macedonia.
INSERT INTO lcc_range SELECT 'DR','2152','2170.999',concept_id FROM concept WHERE preferred_label='North Macedonian Historical Reference';
INSERT INTO lcc_selector SELECT concept_id,'DR2171' FROM concept WHERE preferred_label='North Macedonian Antiquities';
INSERT INTO lcc_selector SELECT concept_id,'DR2172' FROM concept WHERE preferred_label='North Macedonian Social & Cultural History';
INSERT INTO lcc_range SELECT 'DR','2173','2175',concept_id FROM concept WHERE preferred_label='North Macedonian Ethnography';
INSERT INTO lcc_range SELECT 'DR','2181','2194.999',concept_id FROM concept WHERE preferred_label='North Macedonian Historical Studies';
INSERT INTO lcc_range SELECT 'DR','2195','2196.Z',concept_id FROM concept WHERE preferred_label='North Macedonian Political & Diplomatic History';
INSERT INTO lcc_range SELECT 'DR','2197','2202.5',concept_id FROM concept WHERE preferred_label='Macedonia to 1389';
INSERT INTO lcc_range SELECT 'DR','2205','2227',concept_id FROM concept WHERE preferred_label='Ottoman Macedonia, 1389–1912';
INSERT INTO lcc_range SELECT 'DR','2229','2242',concept_id FROM concept WHERE preferred_label='Macedonia, 1912–1945';
INSERT INTO lcc_range SELECT 'DR','2243','2249',concept_id FROM concept WHERE preferred_label='Socialist Macedonia, 1945–1992';
INSERT INTO lcc_range SELECT 'DR','2250','2256',concept_id FROM concept WHERE preferred_label='Independent North Macedonia, 1992–Present';
INSERT INTO lcc_range SELECT 'DR','2275','2285',concept_id FROM concept WHERE preferred_label='North Macedonian Local History';
INSERT INTO lcc_selector SELECT concept_id,'DR2159' FROM concept WHERE preferred_label='North Macedonian Travel';
INSERT INTO lcc_selector SELECT concept_id,'DR2167' FROM concept WHERE preferred_label='North Macedonian Travel';

COMMIT;

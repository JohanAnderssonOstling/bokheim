BEGIN;

INSERT INTO concept(preferred_label) VALUES
  ('Serbian Social & Cultural History'), ('Serbian Ethnography'),
  ('Serbian Biographies'), ('Serbian Historiography'),
  ('Serbian Political History'), ('Serbian Foreign Relations History'),
  ('Early Serbian History, to 1459'), ('Ottoman Serbia, 1459–1804'),
  ('Serbian Autonomy & Independence, 1804–1918'),
  ('Serbia within Yugoslavia, 1918–1992'),
  ('Serbia and Montenegro, 1992–2006'), ('Serbia since 2006'),
  ('Kosovo History'), ('Vojvodina History'), ('Belgrade History');

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT concept_id,3549,0 FROM concept WHERE preferred_label IN (
  'Serbian Social & Cultural History','Serbian Ethnography','Serbian Biographies',
  'Serbian Historiography','Serbian Political History','Serbian Foreign Relations History',
  'Early Serbian History, to 1459','Ottoman Serbia, 1459–1804',
  'Serbian Autonomy & Independence, 1804–1918',
  'Serbia within Yugoslavia, 1918–1992','Serbia and Montenegro, 1992–2006',
  'Serbia since 2006','Vojvodina History','Belgrade History'
);
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT concept_id,3541,0 FROM concept WHERE preferred_label='Kosovo History';

CREATE TEMP TABLE serbian_range(start_value TEXT,end_value TEXT,label TEXT);
INSERT INTO serbian_range VALUES
 ('1953','1954.Z','Serbian Ethnography'),
 ('1956','1958.Z','Serbian Biographies'),
 ('1961','1962.5.Z','Serbian Historiography'),
 ('1975','1976.Z','Serbian Foreign Relations History'),
 ('1977','1999.5.Z','Early Serbian History, to 1459'),
 ('2000','2005.Z','Ottoman Serbia, 1459–1804'),
 ('2006','2032.5','Serbian Autonomy & Independence, 1804–1918'),
 ('2033','2047.Z','Serbia within Yugoslavia, 1918–1992'),
 ('2048','2053.Z','Serbia and Montenegro, 1992–2006'),
 ('2054','2067.Z','Serbia since 2006'),
 ('2075','2088.23.Z','Kosovo History'),
 ('2090','2101.5.Z','Vojvodina History'),
 ('2106','2124.5.Z','Belgrade History');
INSERT INTO lcc_range(class_letters,start_value,end_value,concept_id)
SELECT 'DR',m.start_value,m.end_value,c.concept_id
FROM serbian_range m JOIN concept c ON c.preferred_label=m.label;

INSERT INTO lcc_selector(concept_id,selector)
SELECT concept_id,'DR1952' FROM concept WHERE preferred_label='Serbian Social & Cultural History';
INSERT INTO lcc_selector(concept_id,selector)
SELECT concept_id,'DR1972' FROM concept WHERE preferred_label='Serbian Political History';

INSERT OR IGNORE INTO lcc_selector(concept_id,selector) VALUES
 (2455,'DR1939'),(2455,'DR1947'),(2455,'DR2075.9'),
 (2455,'DR2090.9'),(2455,'DR2109');

DROP TABLE serbian_range;
COMMIT;

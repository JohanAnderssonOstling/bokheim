PRAGMA foreign_keys=ON;
BEGIN;
CREATE TEMP TABLE _ddc_map(selector TEXT PRIMARY KEY, concept_id INTEGER NOT NULL);
INSERT INTO _ddc_map VALUES
 ('360',5462),
 ('361',5462),
 ('362',5897),
 ('363',5896),
 ('364',5463),
 ('365',5465),
 ('366',52374),
 ('367',13216),
 ('368',5230),
 ('369',52374);
INSERT OR REPLACE INTO ddc_selector(selector,concept_id)
SELECT selector,concept_id FROM _ddc_map;
DROP TABLE _ddc_map;
COMMIT;

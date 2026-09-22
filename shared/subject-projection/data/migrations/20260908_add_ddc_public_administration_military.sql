PRAGMA foreign_keys=ON;
BEGIN;
CREATE TEMP TABLE _ddc_map(selector TEXT PRIMARY KEY, concept_id INTEGER NOT NULL);
INSERT INTO _ddc_map VALUES
 ('350',5691),
 ('351',52397),
 ('352',52397),
 ('353',52397),
 ('354',52397),
 ('355',5254),
 ('356',5848),
 ('357',5848),
 ('358',5849),
 ('359',13770);
INSERT OR REPLACE INTO ddc_selector(selector,concept_id)
SELECT selector,concept_id FROM _ddc_map;
DROP TABLE _ddc_map;
COMMIT;

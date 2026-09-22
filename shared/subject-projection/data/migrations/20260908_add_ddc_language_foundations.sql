PRAGMA foreign_keys=ON;
BEGIN;
CREATE TEMP TABLE _ddc_map(selector TEXT PRIMARY KEY, concept_id INTEGER NOT NULL);
INSERT INTO _ddc_map VALUES
 ('400',3702),
 ('401',3778),
 ('402',3733),
 ('403',3734),
 ('404',3702),
 ('405',3702),
 ('406',3832),
 ('407',3832),
 ('408',5536),
 ('409',3780),
 ('410',3778),
 ('411',3704),
 ('412',3779),
 ('413',3770),
 ('414',3782),
 ('415',3750),
 ('417',3780),
 ('418',53474),
 ('419',3829);
INSERT OR REPLACE INTO ddc_selector(selector,concept_id)
SELECT selector,concept_id FROM _ddc_map;
DROP TABLE _ddc_map;
COMMIT;

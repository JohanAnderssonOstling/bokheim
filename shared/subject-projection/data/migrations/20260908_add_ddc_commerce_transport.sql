PRAGMA foreign_keys=ON;
BEGIN;
CREATE TEMP TABLE _ddc_map(selector TEXT PRIMARY KEY, concept_id INTEGER NOT NULL);
INSERT INTO _ddc_map VALUES
 ('380',5083),
 ('381',12269),
 ('382',5182),
 ('383',498),
 ('384',504),
 ('385',480),
 ('386',509),
 ('387',5953),
 ('388',458),
 ('389',1684);
INSERT OR REPLACE INTO ddc_selector(selector,concept_id)
SELECT selector,concept_id FROM _ddc_map;
DROP TABLE _ddc_map;
COMMIT;

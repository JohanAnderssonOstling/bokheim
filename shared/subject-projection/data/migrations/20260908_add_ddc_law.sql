PRAGMA foreign_keys=ON;
BEGIN;
CREATE TEMP TABLE _ddc_map(selector TEXT PRIMARY KEY, concept_id INTEGER NOT NULL);
INSERT INTO _ddc_map VALUES
 ('340',5276),
 ('341',5293),
 ('342',5548),
 ('343',5551),
 ('344',5560),
 ('345',5283),
 ('346',5313),
 ('347',5554),
 ('348',51562),
 ('349',5276);
INSERT OR REPLACE INTO ddc_selector(selector,concept_id)
SELECT selector,concept_id FROM _ddc_map;
DROP TABLE _ddc_map;
COMMIT;

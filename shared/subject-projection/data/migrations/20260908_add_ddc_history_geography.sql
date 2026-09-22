PRAGMA foreign_keys=ON;
BEGIN;
CREATE TEMP TABLE _ddc_map(selector TEXT PRIMARY KEY, concept_id INTEGER NOT NULL);
INSERT INTO _ddc_map VALUES
 ('900',3319),('901',10287),('902',3516),('903',13686),('904',10472),('905',13686),('906',10390),('907',3684),('908',3616),('909',3701),
 ('910',2382),('911',2382),('912',2369),('913',6616),('914',2455),('915',2433),('916',2427),('917',2426),('918',2494),('919',2444);
INSERT OR REPLACE INTO ddc_selector(selector,concept_id)
SELECT selector,concept_id FROM _ddc_map;
DROP TABLE _ddc_map;
COMMIT;

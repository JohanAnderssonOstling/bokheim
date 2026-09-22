PRAGMA foreign_keys=ON;
BEGIN;
CREATE TEMP TABLE _ddc_map(selector TEXT PRIMARY KEY, concept_id INTEGER NOT NULL);
INSERT INTO _ddc_map VALUES
 ('370',2169),
 ('371',2257),
 ('372',2322),
 ('373',2327),
 ('374',2175),
 ('375',2188),
 ('377',2291),
 ('378',2321),
 ('379',2196);
INSERT OR REPLACE INTO ddc_selector(selector,concept_id)
SELECT selector,concept_id FROM _ddc_map;
DROP TABLE _ddc_map;
COMMIT;

PRAGMA foreign_keys=ON;
BEGIN;
CREATE TEMP TABLE _ddc_map(selector TEXT PRIMARY KEY, concept_id INTEGER NOT NULL);
INSERT INTO _ddc_map VALUES
 ('330',5137),
 ('331',5234),
 ('332',5142),
 ('333',13614),
 ('334',12338),
 ('335',11761),
 ('336',5209),
 ('337',52758),
 ('338',13620),
 ('339',5593);
INSERT OR REPLACE INTO ddc_selector(selector,concept_id)
SELECT selector,concept_id FROM _ddc_map;
DROP TABLE _ddc_map;
COMMIT;

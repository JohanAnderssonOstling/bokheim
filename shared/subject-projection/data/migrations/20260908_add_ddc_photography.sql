PRAGMA foreign_keys=ON;
BEGIN;
CREATE TEMP TABLE _ddc_map(selector TEXT PRIMARY KEY, concept_id INTEGER NOT NULL);
INSERT INTO _ddc_map VALUES
 ('770',915),('771',967),('772',9436),('773',9436),('774',330),('775',53487),('776',53487),('777',600),('778',966),('779',5855);
INSERT OR REPLACE INTO ddc_selector(selector,concept_id)
SELECT selector,concept_id FROM _ddc_map;
DROP TABLE _ddc_map;
COMMIT;

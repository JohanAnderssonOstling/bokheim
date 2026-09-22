PRAGMA foreign_keys=ON;
BEGIN;
CREATE TEMP TABLE _ddc_map(selector TEXT PRIMARY KEY, concept_id INTEGER NOT NULL);
INSERT INTO _ddc_map VALUES
 ('740',647),('741',662),('742',9470),('743',673),('745',650),('746',6600),('747',655),('748',690),('749',653),
 ('750',1032),('751',6931),('752',8981),('753',9495),('754',1037),('755',1037),('757',7485),('758',9312),('759',1034);
INSERT OR REPLACE INTO ddc_selector(selector,concept_id)
SELECT selector,concept_id FROM _ddc_map;
DROP TABLE _ddc_map;
COMMIT;

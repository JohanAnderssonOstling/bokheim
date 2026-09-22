PRAGMA foreign_keys=ON;
BEGIN;
CREATE TEMP TABLE _ddc_map(selector TEXT PRIMARY KEY, concept_id INTEGER NOT NULL);
INSERT INTO _ddc_map VALUES
 ('500',4779),('501',4779),('502',4779),('503',4779),('505',5726),('506',5726),('507',4899),('508',4879),('509',1682),
 ('510',4916),('511',5716),('512',4917),('513',4925),('514',4971),('515',5719),('516',4939),('518',9005),('519',4961);
INSERT OR REPLACE INTO ddc_selector(selector,concept_id)
SELECT selector,concept_id FROM _ddc_map;
DROP TABLE _ddc_map;
COMMIT;

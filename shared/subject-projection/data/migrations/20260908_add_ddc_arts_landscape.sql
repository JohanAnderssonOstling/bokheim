PRAGMA foreign_keys=ON;
BEGIN;
CREATE TEMP TABLE _ddc_map(selector TEXT PRIMARY KEY, concept_id INTEGER NOT NULL);
INSERT INTO _ddc_map VALUES
 ('700',510),('701',11215),('702',969),('703',969),('704',1015),('705',969),('706',1031),('707',1065),('708',1031),('709',1015),
 ('710',5700),('711',11780),('712',5700),('713',5700),('714',13106),('715',9843),('716',12344),('717',13100),('718',12606),('719',587);
INSERT OR REPLACE INTO ddc_selector(selector,concept_id)
SELECT selector,concept_id FROM _ddc_map;
DROP TABLE _ddc_map;
COMMIT;

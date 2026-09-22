PRAGMA foreign_keys=ON;
BEGIN;
CREATE TEMP TABLE _ddc_map(selector TEXT PRIMARY KEY, concept_id INTEGER NOT NULL);
INSERT INTO _ddc_map VALUES
 ('600',1),('601',5563),('602',5563),('603',5563),('604',5563),('605',5563),('606',5563),('607',5563),('608',347),('609',329),
 ('610',2535),('611',4912),('612',5043),('613',53231),('614',2845),('615',2888),('616',2597),('617',2854),('618',2613),('619',5580);
INSERT OR REPLACE INTO ddc_selector(selector,concept_id)
SELECT selector,concept_id FROM _ddc_map;
DROP TABLE _ddc_map;
COMMIT;

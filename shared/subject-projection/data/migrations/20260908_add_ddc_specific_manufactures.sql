PRAGMA foreign_keys=ON;
BEGIN;
CREATE TEMP TABLE _ddc_map(selector TEXT PRIMARY KEY, concept_id INTEGER NOT NULL);
INSERT INTO _ddc_map VALUES
 ('680',12205),('681',9491),('682',353),('683',361),('684',12205),('685',52614),('686',4686),('687',52622),('688',354);
INSERT OR REPLACE INTO ddc_selector(selector,concept_id)
SELECT selector,concept_id FROM _ddc_map;
DROP TABLE _ddc_map;
COMMIT;

PRAGMA foreign_keys=ON;
BEGIN;
-- 132, 134 and 136 are unassigned in DDC.
INSERT OR REPLACE INTO ddc_selector(selector,concept_id) VALUES
 ('130',3310),('131',5654),('133',3294),('135',3277),
 ('137',5405),('138',5428),('139',5427);
COMMIT;

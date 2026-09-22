PRAGMA foreign_keys=ON;
BEGIN;
-- 256-258 are unassigned in DDC.
INSERT OR REPLACE INTO ddc_selector(selector,concept_id) VALUES
 ('250',4569),('251',4620),('252',51354),('253',11587),
 ('254',4617),('255',4565),('259',4576);
COMMIT;

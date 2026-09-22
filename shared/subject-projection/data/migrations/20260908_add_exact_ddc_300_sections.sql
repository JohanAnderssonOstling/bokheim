PRAGMA foreign_keys=ON;
BEGIN;
-- 308 and 309 are unassigned in DDC.
INSERT OR REPLACE INTO ddc_selector(selector,concept_id) VALUES
 ('300',5055),('301',5055),('302',8858),('303',5665),
 ('304',12260),('305',5661),('306',5662),('307',12261);
COMMIT;

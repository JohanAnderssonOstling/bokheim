PRAGMA foreign_keys=ON;
BEGIN;
-- 104 is unassigned in DDC.
INSERT OR REPLACE INTO ddc_selector(selector,concept_id) VALUES
 ('100',4487),('101',4487),
 ('102',5532),('103',5532),('105',5532),('106',5532),('107',5532),
 ('108',4487),('109',4507);
COMMIT;

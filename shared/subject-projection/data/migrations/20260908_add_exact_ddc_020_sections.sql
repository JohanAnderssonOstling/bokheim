PRAGMA foreign_keys=ON;
BEGIN;

-- 020 library and information science. 024 and 029 are unassigned in DDC.
INSERT OR REPLACE INTO ddc_selector(selector,concept_id) VALUES
 ('020',4748),
 ('021',6148),
 ('022',4675),
 ('023',4766),
 ('025',4762),
 ('026',3773),
 ('027',6109),
 ('028',4322);

COMMIT;

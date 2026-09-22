PRAGMA foreign_keys=ON;
BEGIN;
-- 216, 217 and 219 are unassigned in DDC. The missing 213-215 concepts await
-- explicit curation approval.
INSERT OR REPLACE INTO ddc_selector(selector,concept_id) VALUES
 ('210',4538),('211',12998),('212',12998),('218',4538);
COMMIT;

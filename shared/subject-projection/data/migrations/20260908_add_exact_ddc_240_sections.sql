PRAGMA foreign_keys=ON;
BEGIN;
-- 244 and 245 are unassigned in DDC.
INSERT OR REPLACE INTO ddc_selector(selector,concept_id) VALUES
 ('240',4619),('241',12502),('242',5907),('243',11588),
 ('246',6263),('247',11640),('248',11588),('249',12498);
COMMIT;

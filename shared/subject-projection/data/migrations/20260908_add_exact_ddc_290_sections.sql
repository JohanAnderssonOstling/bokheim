PRAGMA foreign_keys=ON;
BEGIN;
-- 291 and 298 are unassigned in the current DDC summary.
INSERT OR REPLACE INTO ddc_selector(selector,concept_id) VALUES
 ('290',5859),('292',12852),('293',12853),('294',4621),
 ('295',4627),('296',4615),('297',5859),('299',5859);
COMMIT;

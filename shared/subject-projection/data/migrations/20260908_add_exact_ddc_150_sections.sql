PRAGMA foreign_keys=ON;
BEGIN;
-- 151, 157 and 159 are unassigned in DDC.
INSERT OR REPLACE INTO ddc_selector(selector,concept_id) VALUES
 ('150',5381),('152',5389),('153',5389),('154',53287),
 ('155',5391),('156',5383),('158',5384);
COMMIT;

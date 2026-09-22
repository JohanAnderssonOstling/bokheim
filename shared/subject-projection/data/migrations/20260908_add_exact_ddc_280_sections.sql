PRAGMA foreign_keys=ON;
BEGIN;
-- 288 is unassigned in DDC. Combined denominational sections remain on their
-- common curated parent rather than favoring one constituent denomination.
INSERT OR REPLACE INTO ddc_selector(selector,concept_id) VALUES
 ('280',4558),('281',4563),('282',4584),('283',11478),('284',4566),
 ('285',4566),('286',4566),('287',11484),('289',4562);
COMMIT;

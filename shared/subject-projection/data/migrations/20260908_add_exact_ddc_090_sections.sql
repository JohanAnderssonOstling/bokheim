PRAGMA foreign_keys=ON;
BEGIN;
-- Manuscripts and special books. Broad residual sections remain on the closest
-- existing curated book-history parent instead of inventing artificial leaves.
INSERT OR REPLACE INTO ddc_selector(selector,concept_id) VALUES
 ('090',4690),
 ('091',4692),
 ('092',4684),
 ('093',7360),
 ('094',3046),
 ('095',4680),
 ('096',7271),
 ('097',4720),
 ('098',4720),
 ('099',4720);
COMMIT;

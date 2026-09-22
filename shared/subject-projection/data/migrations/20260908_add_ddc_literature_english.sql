PRAGMA foreign_keys=ON;
BEGIN;
CREATE TEMP TABLE _ddc_map(selector TEXT PRIMARY KEY, concept_id INTEGER NOT NULL);
INSERT INTO _ddc_map VALUES
 ('800',3846),('801',4415),('802',52309),('803',52309),('805',52309),('806',52309),('807',52309),('808',4280),('809',4309),
 ('810',3853),('811',6915),('812',7616),('813',8219),('814',7953),('815',9356),('816',8777),('817',3853),('818',9689),
 ('820',5585),('821',9208),('822',9057),('823',9089),('824',6623),('825',7827),('826',7641),('827',3909),('828',6218),('829',3911);
INSERT OR REPLACE INTO ddc_selector(selector,concept_id)
SELECT selector,concept_id FROM _ddc_map;
DROP TABLE _ddc_map;
COMMIT;

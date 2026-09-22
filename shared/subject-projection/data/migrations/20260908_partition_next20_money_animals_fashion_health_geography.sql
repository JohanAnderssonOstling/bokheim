-- Next-20 review: move only official, specific contiguous subranges into
-- established children. General and residual material remains at the parent.
BEGIN;

-- Money history within Money & Liquidity.
DELETE FROM lcc_range WHERE concept_id=12837
 AND start_code='HG230.71' AND end_code='HG257.99';
INSERT OR IGNORE INTO lcc_range(start_code,end_code,concept_id) VALUES
 ('HG230.71','HG230.99',12837),('HG231','HG256',52772),
 ('HG256.1','HG257.99',12837);

-- Dog breeds are a Cutter interval, not a generated list of exact codes.
DELETE FROM lcc_range WHERE concept_id=3152
 AND start_code='SF429' AND end_code='SF429';
INSERT OR IGNORE INTO lcc_range(start_code,end_code,concept_id)
VALUES('SF429.A','SF429.Z',3152);

-- Animal stories and anecdotes.
DELETE FROM lcc_range WHERE concept_id=5050
 AND start_code='QL750' AND end_code='QL795';
INSERT OR IGNORE INTO lcc_range(start_code,end_code,concept_id) VALUES
 ('QL750','QL790.99',5050),('QL791','QL795',9836);

-- Fashion by period/region and material topic.
DELETE FROM lcc_range WHERE concept_id=676
 AND start_code='GT500' AND end_code='GT2370';
INSERT OR IGNORE INTO lcc_range(start_code,end_code,concept_id) VALUES
 ('GT500','GT529.99',676),('GT530','GT560',52624),
 ('GT560.1','GT622.99',676),('GT623','GT716',52626),
 ('GT716.1','GT719.99',676),('GT720','GT1330',52627),
 ('GT1330.1','GT1369.99',676),('GT1370','GT1570',52628),
 ('GT1570.1','GT2249.99',676),('GT2250','GT2282',52631),
 ('GT2282.1','GT2289.99',676),('GT2290','GT2320',52632),
 ('GT2320.1','GT2370',676);

-- Psychopathology. Conditions with established children get their official
-- schedule blocks; diagnostic/general gaps stay on Psychopathology.
DELETE FROM lcc_range WHERE concept_id=2715;
INSERT OR IGNORE INTO lcc_range(start_code,end_code,concept_id) VALUES
 ('RC512','RC513.99',2715),('RC514','RC514',12661),
 ('RC514.1','RC515.99',2715),('RC516','RC516',12662),
 ('RC516.1','RC520.99',2715),('RC521','RC524',12663),
 ('RC524.1','RC529.99',2715),('RC530','RC530.99',2715),
 ('RC531','RC531',12664),('RC531.1','RC532.99',2715),
 ('RC533','RC533',12665),('RC533.1','RC534.99',2715),
 ('RC535','RC535',12664),('RC535.1','RC536.99',2715),
 ('RC537','RC545',12666),('RC545.1','RC546.99',2715),
 ('RC547','RC549.3',12667),('RC549.31','RC551.99',2715),
 ('RC552.A','RC552.Z',12668),('RC553','RC553.99',2715),
 ('RC554','RC555',12670),('RC555.1','RC555.99',2715),
 ('RC556','RC560',12671),('RC560.1','RC562.99',2715),
 ('RC563','RC568',12672),('RC568.1','RC568.99',2715),
 ('RC569','RC569',12673),('RC569.1','RC569.5',2715);

-- Physical geography.
DELETE FROM lcc_range WHERE concept_id=2421
 AND start_code='GB3' AND end_code='GB5030';
INSERT OR IGNORE INTO lcc_range(start_code,end_code,concept_id) VALUES
 ('GB3','GB399.99',2421),('GB400','GB649',2422),
 ('GB649.1','GB650.99',2421),('GB651','GB2998',2424),
 ('GB2998.1','GB4999.99',2421),('GB5000','GB5030',2425);

-- Hospitality history, biography, and food service.
DELETE FROM lcc_range WHERE concept_id=3119;
INSERT OR IGNORE INTO lcc_range(start_code,end_code,concept_id) VALUES
 ('TX901','TX907.99',3119),('TX908','TX910.222',9562),
 ('TX910.223','TX910.299',3119),('TX910.3','TX910.5',6645),
 ('TX910.51','TX942.99',3119),('TX943','TX946.5',6925);

COMMIT;

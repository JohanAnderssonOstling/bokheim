-- Next-20 review: broad official divisions only; general gaps stay at parent.
BEGIN;

-- Architecture.
DELETE FROM lcc_range WHERE concept_id=193
 AND start_code='NA1' AND end_code='NA9428';
INSERT OR IGNORE INTO lcc_range(start_code,end_code,concept_id) VALUES
 ('NA1','NA189.99',193),('NA190','NA1555.5',5702),
 ('NA1555.6','NA1999.99',193),('NA2000','NA2460',5704),
 ('NA2460.1','NA2499.99',193),('NA2500','NA4050',5698),
 ('NA4050.1','NA4099.99',193),('NA4100','NA8480',5699),
 ('NA8480.1','NA8999.99',193),('NA9000','NA9428',5701);

-- Motor vehicles, aeronautics, rockets, and astronautics.
DELETE FROM lcc_range WHERE concept_id=404;
INSERT OR IGNORE INTO lcc_range(start_code,end_code,concept_id) VALUES
 ('TL1','TL484',407),('TL484.1','TL499.99',404),
 ('TL500','TL777',405),('TL777.1','TL779.99',404),
 ('TL780','TL785.8',408),('TL785.81','TL786.99',404),
 ('TL787','TL4050',406);

-- Remove the artificial Anthropology Fields wrapper and retain its real fields
-- directly under Anthropology.
INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT p.concept_id,5056,
 coalesce((SELECT max(x.ordinal)+1 FROM concept_parent x WHERE x.concept_id=p.concept_id),0)
FROM concept_parent p WHERE p.parent_concept_id=5057
 AND NOT EXISTS(SELECT 1 FROM concept_parent x WHERE x.concept_id=p.concept_id AND x.parent_concept_id=5056);
DELETE FROM lcc_selector WHERE concept_id=5057;
DELETE FROM bisac_selector WHERE concept_id=5057;
DELETE FROM source_label WHERE concept_id=5057;
DELETE FROM concept_parent WHERE concept_id=5057 OR parent_concept_id=5057;
DELETE FROM concept WHERE concept_id=5057;

DELETE FROM lcc_range WHERE concept_id=5056;
INSERT OR IGNORE INTO lcc_range(start_code,end_code,concept_id) VALUES
 ('GN1','GN48.99',5056),('GN49','GN298',5070),
 ('GN298.1','GN300.99',5056),('GN301','GN674',5061),
 ('GN674.1','GN699.99',5056),('GN700','GN890',5071);
DELETE FROM lcc_selector WHERE concept_id IN(5061,5070,5071)
 AND selector IN('GN301-GN674','GN49-GN298','GN700-GN890');

-- Economic theory.
DELETE FROM lcc_range WHERE concept_id=5593
 AND start_code='HB1' AND end_code='HB846.8';
INSERT OR IGNORE INTO lcc_range(start_code,end_code,concept_id) VALUES
 ('HB1','HB70.99',5593),('HB71','HB74',5097),
 ('HB75','HB130',5098),('HB131','HB147',5100),
 ('HB147.1','HB200.99',5593),('HB201','HB206',5102),
 ('HB206.1','HB220.99',5593),('HB221','HB236',5101),
 ('HB236.1','HB237.99',5593),('HB238','HB251',5094),
 ('HB251.1','HB521.99',5593),('HB522','HB715',5099),
 ('HB715.1','HB800.99',5593),('HB801','HB843',5095),
 ('HB843.1','HB845.99',5593),('HB846','HB846.8',5103);

COMMIT;

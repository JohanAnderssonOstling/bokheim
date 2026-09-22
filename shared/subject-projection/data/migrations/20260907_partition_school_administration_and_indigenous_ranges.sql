-- Extract established specific divisions; retain general administration and
-- general Indigenous material at their existing parents.
BEGIN;

-- School administration.
DELETE FROM lcc_range WHERE concept_id=2323;
INSERT OR IGNORE INTO lcc_range(start_code,end_code,concept_id) VALUES
 ('LB2801','LB2806.14',2323),('LB2806.15','LB2806.15',12681),
 ('LB2806.16','LB2806.21',2323),('LB2806.22','LB2806.22',12682),
 ('LB2806.23','LB2806.29',2323),('LB2806.3','LB2806.5',12683),
 ('LB2806.51','LB2822.79',2323),('LB2822.8','LB2822.84',12643),
 ('LB2822.85','LB2823.49',2323),('LB2823.5','LB2830.5',12644),
 ('LB2830.51','LB2831.59',2323),('LB2831.6','LB2831.99',12645),
 ('LB2832','LB2844.1',12646),('LB2844.11','LB2844.51',2323),
 ('LB2844.52','LB2844.632',12647),('LB2844.633','LB2864.49',2323),
 ('LB2864.5','LB2866.5',12648),('LB2866.51','LB3011.99',2323),
 ('LB3012','LB3012.4',12649),('LB3012.41','LB3012.99',2323),
 ('LB3013','LB3013.25',12650),('LB3013.26','LB3013.29',2323),
 ('LB3013.3','LB3013.34',12651),('LB3013.35','LB3043.99',2323),
 ('LB3044','LB3048',12652),('LB3048.1','LB3049.99',2323),
 ('LB3050','LB3060.87',12653),('LB3060.88','LB3095',2323);

-- Indigenous peoples of North America. Regional E78 Cutter topics remain
-- exact because they do not form safe contiguous numeric regions.
DELETE FROM lcc_range WHERE concept_id=3398
 AND start_code='E75' AND end_code='E99';
INSERT OR IGNORE INTO lcc_range(start_code,end_code,concept_id) VALUES
 ('E75','E77.79',3398),('E77.8','E77.94',10528),
 ('E77.95','E80.99',3398),('E81','E83.895',8296),
 ('E83.896','E84.99',3398),('E85','E87',10475),
 ('E87.1','E90.99',3398),('E91','E95',10400),
 ('E95.1','E95.99',3398),('E96','E97.65',10561),
 ('E97.66','E98.99',3398),('E99.A','E99.Z',50849);

COMMIT;

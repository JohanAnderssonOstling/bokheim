-- Partition two high-use whole schedules into their established specific
-- children while retaining genuine general/reference gaps at the parent.
BEGIN;

-- Education theory and practice (LB).
DELETE FROM lcc_range WHERE concept_id=2315
 AND start_code='LB1' AND end_code='LB3640';
INSERT OR IGNORE INTO lcc_range(start_code,end_code,concept_id) VALUES
 ('LB1','LB50.99',2315),('LB51','LB885',2329),
 ('LB885.1','LB1024.99',2315),('LB1025','LB1049.99',2330),
 ('LB1050','LB1099.99',2320),('LB1100','LB1100.99',2315),
 ('LB1101','LB1139',2316),('LB1139.2','LB1499',2317),
 ('LB1500','LB1599.99',2322),('LB1600','LB1704.99',2327),
 ('LB1705','LB2286',2318),('LB2286.1','LB2299.99',2315),
 ('LB2300','LB2799.3',2321),('LB2799.31','LB2800.99',2315),
 ('LB2801','LB3095',2323),('LB3095.1','LB3200.99',2315),
 ('LB3201','LB3325',2324),('LB3325.1','LB3400.99',2315),
 ('LB3401','LB3499',2325),('LB3499.1','LB3524.99',2315),
 ('LB3525','LB3575',2328),('LB3575.1','LB3601.99',2315),
 ('LB3602','LB3640',2326);

-- Mechanical engineering and machinery (TJ).
DELETE FROM lcc_range WHERE concept_id=369
 AND ((start_code='TJ1' AND end_code='TJ1570') OR
      (start_code='TJ210.2' AND end_code='TJ211.495'));
INSERT OR IGNORE INTO lcc_range(start_code,end_code,concept_id) VALUES
 ('TJ1','TJ211.99',369),('TJ212','TJ225',371),
 ('TJ225.1','TJ226.99',369),('TJ227','TJ240',5948),
 ('TJ240.1','TJ249.99',369),('TJ250','TJ830',5949),
 ('TJ830.1','TJ835.99',369),('TJ836','TJ935',375),
 ('TJ935.1','TJ939.99',369),('TJ940','TJ940.5',391),
 ('TJ940.51','TJ949.99',369),('TJ950','TJ1030',385),
 ('TJ1030.1','TJ1039.99',369),('TJ1040','TJ1345',5948),
 ('TJ1345.1','TJ1349.99',369),('TJ1350','TJ1418',374),
 ('TJ1418.1','TJ1424.99',369),('TJ1425','TJ1475',376),
 ('TJ1475.1','TJ1479.99',369),('TJ1480','TJ1496',370),
 ('TJ1496.1','TJ1500.99',369),('TJ1501','TJ1519',388),
 ('TJ1519.1','TJ1570',369);

COMMIT;

BEGIN;

-- Move every existing range wholly contained by an official LCC travel span.
UPDATE lcc_range SET concept_id=2461 WHERE class_letters='DA' AND CAST(start_value AS REAL)>=725 AND CAST(end_value AS REAL)<=738;
UPDATE lcc_range SET concept_id=2461 WHERE class_letters='DA' AND CAST(start_value AS REAL)>=850 AND CAST(end_value AS REAL)<=878;
UPDATE lcc_range SET concept_id=2463 WHERE class_letters='DA' AND CAST(start_value AS REAL)>=969 AND CAST(end_value AS REAL)<=988;
UPDATE lcc_range SET concept_id=2456 WHERE class_letters='DB' AND CAST(start_value AS REAL)>=21 AND CAST(end_value AS REAL)<=27.5;
UPDATE lcc_range SET concept_id=13996 WHERE class_letters='DB' AND CAST(start_value AS REAL)>=906.9 AND CAST(end_value AS REAL)<=920.5;
UPDATE lcc_range SET concept_id=13996 WHERE class_letters='DB' AND CAST(start_value AS REAL)>=2000 AND CAST(end_value AS REAL)<=2035;
UPDATE lcc_range SET concept_id=2460 WHERE class_letters='DD' AND CAST(start_value AS REAL)>=314 AND CAST(end_value AS REAL)<=320;
UPDATE lcc_range SET concept_id=2462 WHERE class_letters='DF' AND CAST(start_value AS REAL)>=720.5 AND CAST(end_value AS REAL)<=728;
UPDATE lcc_range SET concept_id=2464 WHERE class_letters='DG' AND CAST(start_value AS REAL)>=27 AND CAST(end_value AS REAL)<=41;
UPDATE lcc_range SET concept_id=2455 WHERE class_letters='DH' AND CAST(start_value AS REAL)>=31 AND CAST(end_value AS REAL)<=40;
UPDATE lcc_range SET concept_id=2455 WHERE class_letters='DJ' AND CAST(start_value AS REAL)>=33 AND CAST(end_value AS REAL)<=41;
UPDATE lcc_range SET concept_id=14021 WHERE class_letters='DL' AND CAST(start_value AS REAL)>=301 AND CAST(end_value AS REAL)<=334;
UPDATE lcc_range SET concept_id=2471 WHERE class_letters='DP' AND CAST(start_value AS REAL)>=27.5 AND CAST(end_value AS REAL)<=43.2;
UPDATE lcc_range SET concept_id=2455 WHERE class_letters='DR' AND CAST(start_value AS REAL)>=11.5 AND CAST(end_value AS REAL)<=16;
UPDATE lcc_range SET concept_id=13996 WHERE class_letters='DR' AND CAST(start_value AS REAL)>=1220 AND CAST(end_value AS REAL)<=1224;
UPDATE lcc_range SET concept_id=2427 WHERE class_letters='DT' AND CAST(start_value AS REAL)>=7 AND CAST(end_value AS REAL)<=12.25;

-- Add the official umbrella spans where only broader History fallbacks existed.
INSERT OR IGNORE INTO lcc_range VALUES
 ('DA','725','738',2461),('DA','850','878',2461),('DA','969','988',2463),
 ('DB','21','27.5',2456),('DB','906.9','920.5',13996),('DB','2000','2035',13996),
 ('DD','314','320',2460),('DF','720.5','728',2462),('DG','27','41',2464),
 ('DH','31','40',2455),('DJ','33','41',2455),('DL','301','334',14021),
 ('DP','27.5','43.2',2471),('DR','11.5','16',2455),('DR','1220','1224',13996),
 ('DT','7','12.25',2427);

COMMIT;

-- Additional terminal subjects whose explicit numbers/cutters occupy one
-- continuous LC block. Sparse topical cutter lists are intentionally omitted.
PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;

DELETE FROM source_selector
WHERE system_id='lcc' AND concept_id IN
 (4413,4463,7308,7354,6746,6801,8114,8433,8887,11392,11257,
  11796,12730,13163,6744,7572,8851,11037,11145,12912);

INSERT INTO source_selector VALUES
 (4413,'lcc','P99.5..P99.7'),
 (4463,'lcc','PG3312..PG3318'),
 (7308,'lcc','RM302..RM302.4'),
 (7354,'lcc','NB170..NB180'),
 (6746,'lcc','NC790..NC800'),
 (6801,'lcc','LB1088..LB1091.5'),
 (8114,'lcc','RA1219.3..RA1220.5'),
 (8433,'lcc','ND2560..ND2575'),
 (8887,'lcc','ND3332..ND3344'),
 (11392,'lcc','DK908.867..DK908.868'),
 (11257,'lcc','F2528..F2532'),
 (11796,'lcc','QA76.8.A662..QA76.8.A66235'),
 (12730,'lcc','HV6763..HV6769'),
 (13163,'lcc','LB2335.3..LB2335.5'),
 (6744,'lcc','ND2578..ND2583'),
 (7572,'lcc','TX951..TX951.3.Z'),
 (8851,'lcc','BJ1291..BJ1292.Z'),
 (11037,'lcc','BF724.6..BF724.65.Z'),
 (11145,'lcc','BJ231'),
 (11145,'lcc','BJ251..BJ255.A3'),
 (12912,'lcc','BX2312..BX2345');

COMMIT;

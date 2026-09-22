BEGIN;

INSERT OR IGNORE INTO lcc_selector(concept_id,selector) VALUES
 (5332,'JV1-JV5397'),
 (5332,'JV500-JV5397'),
 (5332,'JV6001-JV9480'),
 (5332,'JV7590-JV8349'),
 (5332,'JV9472-JV9473');

UPDATE lcc_selector SET concept_id=12439
WHERE concept_id=5332 AND selector IN
 ('JV1-JV9','JV10-JV19','JV21','JV22','JV23','JV51','JV55-JV57','JV60',
  'JV61-JV152','JV185','JV201-JV246','JV305-JV308','JV321','JV331',
  'JV341','JV412-JV461');

UPDATE lcc_selector SET concept_id=12440
WHERE concept_id=5332 AND selector='JV500-JV5397';

UPDATE lcc_selector SET concept_id=12441
WHERE concept_id=5332 AND selector IN
 ('JV6001-JV6006','JV6008','JV6011','JV6012','JV6013','JV6013.5',
  'JV6019-JV6020','JV6021-JV6033','JV6035','JV6038');

UPDATE lcc_selector SET concept_id=12442
WHERE concept_id=5332 AND selector='JV6061-JV6149';

UPDATE lcc_selector SET concept_id=12443
WHERE concept_id=5332 AND selector='JV6198.2-JV6348';

UPDATE lcc_selector SET concept_id=12444
WHERE concept_id=5332 AND selector IN ('JV6350-JV7539','JV7200-JV7539');

UPDATE lcc_selector SET concept_id=12445
WHERE concept_id=5332 AND selector='JV7590-JV8349';

UPDATE lcc_selector SET concept_id=12446
WHERE concept_id=5332 AND selector IN ('JV8490-JV8758','JV8760','JV8762');

UPDATE lcc_selector SET concept_id=12447
WHERE concept_id=5332 AND selector='JV8790-JV9025';

UPDATE lcc_selector SET concept_id=12448
WHERE concept_id=5332 AND selector IN ('JV9029-JV9036','JV9040-JV9047');

UPDATE lcc_selector SET concept_id=12449
WHERE concept_id=5332 AND selector IN
 ('JV9100-JV9299','JV9260-JV9269','JV9290-JV9470');

UPDATE lcc_selector SET concept_id=12450
WHERE concept_id=5332 AND selector IN ('JV9472-JV9473','JV9475','JV9480');

DELETE FROM lcc_range WHERE concept_id=5332;

COMMIT;

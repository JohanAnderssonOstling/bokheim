BEGIN;

UPDATE lcc_selector SET concept_id=12420
WHERE concept_id=5255 AND selector='UA21-UA885';

UPDATE lcc_selector SET concept_id=12421
WHERE concept_id=5255 AND selector IN ('UA910','UA910-UA915');

UPDATE lcc_selector SET concept_id=12422
WHERE concept_id=5255 AND selector IN
 ('UA920','UA920-UA925','UA923-UA925');

UPDATE lcc_selector SET concept_id=12423
WHERE concept_id=5255 AND selector IN
 ('UA926-UA929','UA926.5','UA929.5','UA929.6-UA929.9');

UPDATE lcc_selector SET concept_id=12424
WHERE concept_id=5255 AND selector IN
 ('UA940','UA940-UA945','UA943-UA945');

UPDATE lcc_selector SET concept_id=12425
WHERE concept_id=5255 AND selector IN
 ('UA950','UA950-UA979','UA953-UA955','UA960-UA965','UA970-UA975');

UPDATE lcc_selector SET concept_id=12426
WHERE concept_id=5255 AND selector IN
 ('UA985','UA985-UA997','UA990','UA993-UA995','UA997');

COMMIT;

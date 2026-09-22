BEGIN;

UPDATE lcc_selector SET concept_id=12427
WHERE concept_id=5231 AND selector IN
 ('HD1428','HD1428-HD1431','HD1429','HD1431');

UPDATE lcc_selector SET concept_id=12428
WHERE concept_id=5231 AND selector IN
 ('HD1470','HD1470-HD1476','HD1483','HD1483-HD1486','HD1484-HD1486',
  'HD1491-HD1491.5','HD1491.5','HD1492-HD1492.5','HD1492.5',
  'HD1493-HD1493.5','HD1493.5');

UPDATE lcc_selector SET concept_id=12429
WHERE concept_id=5231 AND selector='HD1549';

UPDATE lcc_selector SET concept_id=12430
WHERE concept_id=5231 AND selector='HD1580';

COMMIT;

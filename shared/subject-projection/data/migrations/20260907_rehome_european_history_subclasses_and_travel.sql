BEGIN;

UPDATE source_selector SET concept_id=3614
WHERE concept_id=3517 AND system_id='lcc' AND selector='DP';

UPDATE source_selector SET concept_id=3522
WHERE concept_id=3517 AND system_id='lcc' AND selector='DK';

UPDATE source_selector SET concept_id=3607
WHERE concept_id=3517 AND system_id='lcc' AND selector='DG6';

UPDATE source_selector SET concept_id=3552
WHERE concept_id=3517 AND system_id='lcc' AND selector='DB9992';

UPDATE lcc_range SET concept_id=2455
WHERE concept_id=3517 AND class_letters='D'
  AND start_value='901' AND end_value='980';

COMMIT;

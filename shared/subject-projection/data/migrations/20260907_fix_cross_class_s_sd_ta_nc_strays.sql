BEGIN;

UPDATE lcc_selector SET concept_id=2
WHERE concept_id=13337 AND selector='S403';

UPDATE lcc_selector SET concept_id=25
WHERE concept_id=13379 AND selector='SD389';

DELETE FROM lcc_range
WHERE concept_id=13380 AND start_code LIKE 'SD%';

UPDATE lcc_selector SET concept_id=5563
WHERE concept_id=13493 AND selector='TA495';

UPDATE lcc_selector SET concept_id=1009
WHERE concept_id=13557 AND selector='NC930';

COMMIT;

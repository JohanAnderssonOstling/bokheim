BEGIN;

UPDATE lcc_selector SET concept_id=11918
WHERE concept_id=5691 AND selector='JL480-JL489';

UPDATE lcc_selector SET concept_id=11924
WHERE concept_id=5691 AND selector='JL3100.2-JL3102.2';

UPDATE lcc_selector SET concept_id=11618
WHERE concept_id=5691 AND selector='JL599.2';

COMMIT;

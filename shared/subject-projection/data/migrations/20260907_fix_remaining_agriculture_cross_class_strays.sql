BEGIN;

UPDATE lcc_selector SET concept_id=4854
WHERE concept_id=13456 AND selector='SB319.95';

UPDATE lcc_selector SET concept_id=25
WHERE concept_id=13382 AND selector='SD119';

UPDATE lcc_selector SET concept_id=32
WHERE concept_id=13381 AND selector='SD388.5';

COMMIT;

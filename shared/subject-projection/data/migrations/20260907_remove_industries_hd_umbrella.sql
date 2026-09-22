BEGIN;

DELETE FROM lcc_range
WHERE concept_id=5157 AND start_code='HD28' AND end_code='HD9999';
INSERT OR IGNORE INTO lcc_selector(concept_id,selector)
VALUES (5157,'HD28-9999');
UPDATE lcc_selector SET concept_id=5233
WHERE concept_id=5157 AND selector='HD58';

COMMIT;

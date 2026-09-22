BEGIN;

UPDATE lcc_selector SET concept_id=13107
WHERE concept_id=5232 AND selector IN ('HD72','HD73');

UPDATE lcc_selector SET concept_id=13109
WHERE concept_id=5232 AND selector='HD76-HD76.2';

UPDATE lcc_selector SET concept_id=13110
WHERE concept_id=5232 AND selector='HD77-HD77.5';

UPDATE lcc_selector SET concept_id=13111
WHERE concept_id=5232 AND selector='HD78';

UPDATE lcc_selector SET concept_id=13112
WHERE concept_id=5232 AND selector='HD82-HD85';

UPDATE lcc_selector SET concept_id=13113
WHERE concept_id=5232 AND selector='HD87-HD87.55';

INSERT OR IGNORE INTO lcc_selector(concept_id,selector)
VALUES (5232,'HD72-HD88');

DELETE FROM lcc_range
WHERE concept_id=5232 AND start_code='HD72' AND end_code='HD88';

COMMIT;

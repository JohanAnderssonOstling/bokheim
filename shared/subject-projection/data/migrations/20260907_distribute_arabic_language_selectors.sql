BEGIN;

UPDATE concept
SET preferred_label='Arabic Grammar, Rhetoric & Translation'
WHERE concept_id=12869;

-- Correct the generated boundary: morphology begins at PJ6131, so phonology
-- must not permissively absorb it.
DELETE FROM lcc_range WHERE concept_id IN (3713,12868,12869);
INSERT OR IGNORE INTO lcc_range VALUES('PJ','6121','6129.999',12868);
INSERT OR IGNORE INTO lcc_range VALUES('PJ','6131','6171.9',12869);

UPDATE lcc_selector SET concept_id=12867
WHERE concept_id=3713 AND selector IN
 ('PJ6101','PJ6106','PJ6111','PJ6115','PJ6119','PJ6119.5','PJ6120');

UPDATE lcc_selector SET concept_id=12869
WHERE concept_id=3713 AND selector IN
 ('PJ6131','PJ6151','PJ6161','PJ6166','PJ6167','PJ6170','PJ6171');

UPDATE lcc_selector SET concept_id=12870
WHERE concept_id=3713 AND selector='PJ6199.5';

COMMIT;

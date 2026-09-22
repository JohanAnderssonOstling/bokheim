BEGIN;

-- Preserve the printed umbrella notation as an exact parent selector, while
-- routing named notations to the already-existing semantic children.
INSERT OR IGNORE INTO lcc_selector(concept_id,selector)
VALUES (5099,'HB522-HB715');

UPDATE lcc_selector SET concept_id=13128
WHERE concept_id=5099 AND selector IN ('HB522','HB523','HB525','HC79.I5');

UPDATE lcc_selector SET concept_id=13131
WHERE concept_id=5099 AND selector='HB601';

UPDATE lcc_selector SET concept_id=13132
WHERE concept_id=5099 AND selector='HB615';

UPDATE lcc_selector SET concept_id=13134
WHERE concept_id=5099 AND selector='HC79.W4';

-- The child ranges already partition HB522-HB715 without gaps.
DELETE FROM lcc_range
WHERE concept_id=5099 AND start_code='HB522' AND end_code='HB715';

COMMIT;

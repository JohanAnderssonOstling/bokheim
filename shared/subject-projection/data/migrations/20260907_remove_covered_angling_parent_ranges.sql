-- Angling's five children already partition SH401-SH691 completely.
-- Preserve printed umbrella/subrange notations as exact selectors only.
BEGIN;

INSERT OR IGNORE INTO lcc_selector(concept_id,selector)
SELECT concept_id,start_code||'-'||end_code
FROM lcc_range
WHERE concept_id=21;

DELETE FROM lcc_range
WHERE concept_id=21;

COMMIT;

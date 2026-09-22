-- Language/region children contain every detailed epigraphy interval.
BEGIN;

INSERT OR IGNORE INTO lcc_selector(concept_id,selector)
SELECT concept_id,start_code||'-'||end_code
FROM lcc_range
WHERE concept_id=3628 AND NOT (start_code='CN1' AND end_code='CN1355');

DELETE FROM lcc_range
WHERE concept_id=3628 AND NOT (start_code='CN1' AND end_code='CN1355');

COMMIT;

-- Fisheries children contain every detailed interval.
BEGIN;

INSERT OR IGNORE INTO lcc_selector(concept_id,selector)
SELECT concept_id,start_code||'-'||end_code
FROM lcc_range
WHERE concept_id=24 AND NOT (start_code='SH201' AND end_code='SH399');

DELETE FROM lcc_range
WHERE concept_id=24 AND NOT (start_code='SH201' AND end_code='SH399');

COMMIT;

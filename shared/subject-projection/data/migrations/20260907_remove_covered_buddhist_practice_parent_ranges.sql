-- Practice/ritual children contain every detailed interval.
BEGIN;

INSERT OR IGNORE INTO lcc_selector(concept_id,selector)
SELECT concept_id,start_code||'-'||end_code
FROM lcc_range
WHERE concept_id=5754 AND NOT (start_code='BQ4911' AND end_code='BQ5720');

DELETE FROM lcc_range
WHERE concept_id=5754 AND NOT (start_code='BQ4911' AND end_code='BQ5720');

COMMIT;

BEGIN;

INSERT OR IGNORE INTO lcc_selector(concept_id,selector)
SELECT concept_id,start_code||'-'||end_code
FROM lcc_range
WHERE concept_id=6519 AND start_code='QA401' AND end_code='QA425';

DELETE FROM lcc_range
WHERE concept_id=6519 AND start_code='QA401' AND end_code='QA425';

COMMIT;

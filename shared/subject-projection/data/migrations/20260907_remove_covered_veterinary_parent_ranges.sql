-- Specialty children contain 25 Veterinary Medicine intervals.
BEGIN;

INSERT OR IGNORE INTO lcc_selector(concept_id,selector)
SELECT concept_id,start_code||'-'||end_code
FROM lcc_range
WHERE concept_id=18
  AND (start_code,end_code) NOT IN (VALUES ('SF600','SF1100'),('SF951','SF997.5'));

DELETE FROM lcc_range
WHERE concept_id=18
  AND (start_code,end_code) NOT IN (VALUES ('SF600','SF1100'),('SF951','SF997.5'));

COMMIT;

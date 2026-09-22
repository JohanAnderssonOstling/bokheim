-- Direct children contain every detailed Societies range. Retain only the
-- class-wide and full-schedule umbrellas as permissive parent ranges.
BEGIN;

INSERT OR IGNORE INTO lcc_selector(concept_id,selector)
SELECT concept_id,start_code||'-'||end_code
FROM lcc_range
WHERE concept_id=5517
  AND (start_code,end_code) NOT IN (VALUES ('HS','HS'),('HS1','HS3371'));

DELETE FROM lcc_range
WHERE concept_id=5517
  AND (start_code,end_code) NOT IN (VALUES ('HS','HS'),('HS1','HS3371'));

COMMIT;

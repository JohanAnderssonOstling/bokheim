-- Regional children already contain 106 of the 109 Local Government ranges.
-- Keep only the two broad uncovered umbrellas and LC's printed Tonga span.
BEGIN;

INSERT OR IGNORE INTO lcc_selector(concept_id,selector)
SELECT concept_id,start_code||'-'||end_code
FROM lcc_range
WHERE concept_id=5359
  AND (start_code,end_code) NOT IN (
      VALUES ('JS300','JS1583'),
             ('JS3000','JS6949.89'),
             ('JS8474','JS84749')
  );

DELETE FROM lcc_range
WHERE concept_id=5359
  AND (start_code,end_code) NOT IN (
      VALUES ('JS300','JS1583'),
             ('JS3000','JS6949.89'),
             ('JS8474','JS84749')
  );

COMMIT;

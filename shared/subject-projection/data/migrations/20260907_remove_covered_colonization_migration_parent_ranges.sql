-- Children contain 21 detailed Colonization & Migration intervals.
BEGIN;

CREATE TEMP TABLE _uncovered(start_code TEXT,end_code TEXT,PRIMARY KEY(start_code,end_code));
INSERT INTO _uncovered VALUES
 ('JV1','JV5397'),
 ('JV500','JV5397'),
 ('JV6001','JV9480'),
 ('JV7590','JV8349'),
 ('JV9472','JV9473');

INSERT OR IGNORE INTO lcc_selector(concept_id,selector)
SELECT concept_id,start_code||'-'||end_code
FROM lcc_range
WHERE concept_id=5332
  AND (start_code,end_code) NOT IN (SELECT start_code,end_code FROM _uncovered);

DELETE FROM lcc_range
WHERE concept_id=5332
  AND (start_code,end_code) NOT IN (SELECT start_code,end_code FROM _uncovered);

DROP TABLE _uncovered;
COMMIT;

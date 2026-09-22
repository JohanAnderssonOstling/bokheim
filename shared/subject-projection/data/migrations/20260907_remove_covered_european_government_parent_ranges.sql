-- Country children contain 44 European Government ranges. Preserve the
-- sixteen uncovered cross-national and country-gap intervals.
BEGIN;

CREATE TEMP TABLE _uncovered(start_code TEXT,end_code TEXT,PRIMARY KEY(start_code,end_code));
INSERT INTO _uncovered VALUES
 ('JN1','JN9692.2'),
 ('JN1111','JN1129'),
 ('JN1150','JN1159'),
 ('JN117','JN118'),
 ('JN1170','JN1179'),
 ('JN1187','JN1371'),
 ('JN131','JN231'),
 ('JN307.2','JN841'),
 ('JN3241','JN3415.222'),
 ('JN3445','JN3753'),
 ('JN3770','JN3901'),
 ('JN3925','JN3946'),
 ('JN3951','JN3970'),
 ('JN4000','JN4980'),
 ('JN851','JN865'),
 ('JN900','JN1088');

INSERT OR IGNORE INTO lcc_selector(concept_id,selector)
SELECT concept_id,start_code||'-'||end_code
FROM lcc_range
WHERE concept_id=5368
  AND (start_code,end_code) NOT IN (SELECT start_code,end_code FROM _uncovered);

DELETE FROM lcc_range
WHERE concept_id=5368
  AND (start_code,end_code) NOT IN (SELECT start_code,end_code FROM _uncovered);

DROP TABLE _uncovered;
COMMIT;

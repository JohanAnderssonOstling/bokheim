BEGIN;

CREATE TEMP TABLE _uncovered(concept_id INTEGER,start_code TEXT,end_code TEXT,PRIMARY KEY(concept_id,start_code,end_code));
INSERT INTO _uncovered VALUES
 (5068,'GT','GT'),(5068,'GT1','GT7070'),(5068,'GT1710','GT1956'),(5068,'GT2050','GT2370'),
 (5068,'GT3403','GT3406'),(5068,'GT3410','GT3415'),(5068,'GT3420','GT3452'),(5068,'GT3470','GT3471'),
 (5068,'GT3480','GT3481'),(5068,'GT3510','GT3670.5'),(5068,'GT3770','GT3896'),(5068,'GT3910','GT3911.995'),
 (5068,'GT3912','GT3913.95'),(5068,'GT3925','GT4995'),(5068,'GT5010','GT5090'),(5068,'GT509','GT511'),
 (5068,'GT521','GT525'),(5068,'GT530','GT596'),(5068,'GT601','GT1605'),
 (5235,'HD101','HD1395.5'),(5235,'HD1139.2','HD1339.23'),(5235,'HD1361','HD1395.2'),(5235,'HD166','HD1131'),
 (4244,'PT1','PT4897'),(4244,'PT1375','PT1479'),(4244,'PT1379','PT1381'),(4244,'PT1383','PT1385'),(4244,'PT1391','PT1479'),
 (5253,'HQ1101','HQ1104'),(5253,'HQ1101','HQ2030.7'),(5253,'HQ1111','HQ1111.2'),(5253,'HQ1119','HQ1120'),
 (5253,'HQ1121','HQ1154'),(5253,'HQ1161','HQ1172'),(5253,'HQ1176','HQ1178'),(5253,'HQ1180','HQ1186');

INSERT OR IGNORE INTO lcc_selector(concept_id,selector)
SELECT r.concept_id,r.start_code||'-'||r.end_code FROM lcc_range r
WHERE r.concept_id IN (5068,5235,4244,5253)
  AND NOT EXISTS (SELECT 1 FROM _uncovered u WHERE u.concept_id=r.concept_id AND u.start_code=r.start_code AND u.end_code=r.end_code);

DELETE FROM lcc_range
WHERE concept_id IN (5068,5235,4244,5253)
  AND NOT EXISTS (SELECT 1 FROM _uncovered u WHERE u.concept_id=lcc_range.concept_id AND u.start_code=lcc_range.start_code AND u.end_code=lcc_range.end_code);

DROP TABLE _uncovered;
COMMIT;

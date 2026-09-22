BEGIN;

CREATE TEMP TABLE _uncovered(concept_id INTEGER,start_code TEXT,end_code TEXT,PRIMARY KEY(concept_id,start_code,end_code));
INSERT INTO _uncovered VALUES
 (2318,'LB1705','LB2286'),
 (2318,'LB1805','LB2151'),
 (2725,'R','RZ'),
 (2725,'R129','R130.52'),
 (3904,'PT5001','PT5348'),
 (3904,'PT5040','PT5043'),
 (3904,'PT5043.5','PT5044'),
 (4471,'PQ6001','PQ6168'),
 (4471,'PQ6002.2','PQ6005'),
 (4471,'PQ6013','PQ6020'),
 (4477,'PT9201','PT9499'),
 (4477,'PT9213','PT9214.5'),
 (4477,'PT9240','PT9243'),
 (3806,'PJ8991','PJ8999.5'),
 (3806,'PJ8991','PJ9348'),
 (226,'TP677','TP678'),
 (5039,'QC770','QC798'),
 (5066,'GF1','GF900'),
 (5798,'BT819','BT891.2'),
 (10097,'DQ111','DQ123');

INSERT OR IGNORE INTO lcc_selector(concept_id,selector)
SELECT r.concept_id,r.start_code||'-'||r.end_code
FROM lcc_range r
WHERE r.concept_id IN (2318,2725,3904,4471,4477,3806,226,5039,5066,5798,10097)
  AND NOT EXISTS (SELECT 1 FROM _uncovered u WHERE u.concept_id=r.concept_id AND u.start_code=r.start_code AND u.end_code=r.end_code);

DELETE FROM lcc_range
WHERE concept_id IN (2318,2725,3904,4471,4477,3806,226,5039,5066,5798,10097)
  AND NOT EXISTS (SELECT 1 FROM _uncovered u WHERE u.concept_id=lcc_range.concept_id AND u.start_code=lcc_range.start_code AND u.end_code=lcc_range.end_code);

DROP TABLE _uncovered;
COMMIT;

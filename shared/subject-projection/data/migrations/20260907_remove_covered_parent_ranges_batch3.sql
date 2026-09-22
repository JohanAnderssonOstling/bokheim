-- Direct children contain every detailed range on these parents.
BEGIN;

CREATE TEMP TABLE _uncovered(concept_id INTEGER,start_code TEXT,end_code TEXT,PRIMARY KEY(concept_id,start_code,end_code));
INSERT INTO _uncovered VALUES
 (4750,'Z687','Z718.85'),
 (4378,'PN441','PN1009.5'),
 (2292,'LC65','LC245'),
 (5352,'JZ1305','JZ2060'),
 (5232,'HD72','HD88'),
 (5093,'HB3711','HB3840'),
 (5090,'HF1701','HF2701'),
 (4258,'PQ4001','PQ5999');

INSERT OR IGNORE INTO lcc_selector(concept_id,selector)
SELECT r.concept_id,r.start_code||'-'||r.end_code
FROM lcc_range r
WHERE r.concept_id IN (4750,4378,2292,5352,5232,5093,5090,4258)
  AND NOT EXISTS (SELECT 1 FROM _uncovered u WHERE u.concept_id=r.concept_id AND u.start_code=r.start_code AND u.end_code=r.end_code);

DELETE FROM lcc_range
WHERE concept_id IN (4750,4378,2292,5352,5232,5093,5090,4258)
  AND NOT EXISTS (SELECT 1 FROM _uncovered u WHERE u.concept_id=lcc_range.concept_id AND u.start_code=lcc_range.start_code AND u.end_code=lcc_range.end_code);

DROP TABLE _uncovered;
COMMIT;

BEGIN;

CREATE TEMP TABLE _uncovered(concept_id INTEGER,start_code TEXT,end_code TEXT,PRIMARY KEY(concept_id,start_code,end_code));
INSERT INTO _uncovered VALUES
 (5077,'HT101','HT395'),
 (5077,'HT111','HT150'),
 (3725,'PJ4501','PJ5192'),
 (3535,'DF10','DF951'),
 (3728,'PM101','PM7356'),
 (4382,'PN1010','PN1551'),
 (4412,'P101','P410'),
 (5034,'QC501','QC766'),
 (5038,'QC851','QC999'),
 (5873,'BM150','BM449'),
 (9977,'BF441','BF449.5');

INSERT OR IGNORE INTO lcc_selector(concept_id,selector)
SELECT r.concept_id,r.start_code||'-'||r.end_code
FROM lcc_range r
WHERE r.concept_id IN (5077,3725,3535,3728,4382,4412,5034,5038,5873,9977)
  AND NOT EXISTS (SELECT 1 FROM _uncovered u WHERE u.concept_id=r.concept_id AND u.start_code=r.start_code AND u.end_code=r.end_code);

DELETE FROM lcc_range
WHERE concept_id IN (5077,3725,3535,3728,4382,4412,5034,5038,5873,9977)
  AND NOT EXISTS (SELECT 1 FROM _uncovered u WHERE u.concept_id=lcc_range.concept_id AND u.start_code=lcc_range.start_code AND u.end_code=lcc_range.end_code);

DROP TABLE _uncovered;
COMMIT;

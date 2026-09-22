-- Detailed ranges on these parents are contained by direct child ranges.
-- Preserve one uncovered umbrella on each parent.
BEGIN;

CREATE TEMP TABLE _uncovered(
 concept_id INTEGER NOT NULL,
 start_code TEXT NOT NULL,
 end_code TEXT NOT NULL,
 PRIMARY KEY(concept_id,start_code,end_code)
);
INSERT INTO _uncovered VALUES
 (4280,'PN6120.15','PN6120.95'),
 (3724,'PA1000','PA1179'),
 (5389,'BF309','BF499'),
 (5070,'GN49','GN298'),
 (4591,'BX400','BX756'),
 (5875,'BM450','BM518'),
 (5752,'BQ7001','BQ9800');

INSERT OR IGNORE INTO lcc_selector(concept_id,selector)
SELECT r.concept_id,r.start_code||'-'||r.end_code
FROM lcc_range r
WHERE r.concept_id IN (4280,3724,5389,5070,4591,5875,5752)
  AND NOT EXISTS (
      SELECT 1 FROM _uncovered u
      WHERE u.concept_id=r.concept_id
        AND u.start_code=r.start_code AND u.end_code=r.end_code
  );

DELETE FROM lcc_range
WHERE concept_id IN (4280,3724,5389,5070,4591,5875,5752)
  AND NOT EXISTS (
      SELECT 1 FROM _uncovered u
      WHERE u.concept_id=lcc_range.concept_id
        AND u.start_code=lcc_range.start_code AND u.end_code=lcc_range.end_code
  );

DROP TABLE _uncovered;
COMMIT;

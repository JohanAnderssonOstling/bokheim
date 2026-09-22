-- These Business intervals are wholly contained by direct child ranges.
BEGIN;

CREATE TEMP TABLE _covered(start_code TEXT,end_code TEXT,PRIMARY KEY(start_code,end_code));
INSERT INTO _covered VALUES
 ('HF5030','HF5335.22'),
 ('HF5428','HF5429.6'),
 ('HF5429.7','HF5430.6'),
 ('HF5459.5','HF5459.6'),
 ('HF5460','HF5465'),
 ('HF5465.5','HF5467'),
 ('HF5468.5','HF5469.23'),
 ('HF5469.25','HF5469.55'),
 ('HF5482','HF5482.4'),
 ('HF5482.6','HF5482.65'),
 ('HF5482.7','HF5482.75'),
 ('HF5521','HF5541');

INSERT OR IGNORE INTO lcc_selector(concept_id,selector)
SELECT r.concept_id,r.start_code||'-'||r.end_code
FROM lcc_range r JOIN _covered x USING(start_code,end_code)
WHERE r.concept_id=5086;

DELETE FROM lcc_range
WHERE concept_id=5086
  AND (start_code,end_code) IN (SELECT start_code,end_code FROM _covered);

DROP TABLE _covered;
COMMIT;

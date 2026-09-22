-- Compact additional leaf enumerations into published LCC subspans.
PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;

-- Woodwork: retain the distinct official craft blocks rather than bridging gaps.
DELETE FROM source_selector WHERE concept_id=3114 AND system_id='lcc';
INSERT INTO source_selector VALUES
  (3114,'lcc','NK9600..NK9699'),
  (3114,'lcc','NK9700..NK9799'),
  (3114,'lcc','NK9900..NK9900.7'),
  (3114,'lcc','NK9920..NK9925'),
  (3114,'lcc','NK9930'),
  (3114,'lcc','NK9950'),
  (3114,'lcc','NK9955.A..NK9955.Z');

-- Art anatomy: general, human, and animal anatomy are distinct schedule blocks.
DELETE FROM source_selector WHERE concept_id=8725 AND system_id='lcc';
INSERT INTO source_selector VALUES
  (8725,'lcc','NC760'),
  (8725,'lcc','NC765..NC778'),
  (8725,'lcc','NC780..NC783.8');

-- Mathematical logic: consolidate only explicitly bounded subspans.
DELETE FROM source_selector WHERE concept_id=4951 AND system_id='lcc';
INSERT INTO source_selector VALUES
  (4951,'lcc','QA9.2'),
  (4951,'lcc','QA9.25..QA9.37'),
  (4951,'lcc','QA9.4..QA9.5'),
  (4951,'lcc','QA9.54'),
  (4951,'lcc','QA9.56..QA9.59'),
  (4951,'lcc','QA9.6..QA9.63'),
  (4951,'lcc','QA9.64'),
  (4951,'lcc','QA9.65..QA9.67'),
  (4951,'lcc','QA9.7'),
  (4951,'lcc','QA9.8'),
  (4951,'lcc','QA9.9'),
  (4951,'lcc','QA9.A1..QA9.Z'),
  (4951,'lcc','QA10..QA10.3'),
  (4951,'lcc','QA10.35');

COMMIT;

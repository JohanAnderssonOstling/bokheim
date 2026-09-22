-- Redistribute the high-use portions of the remaining broad parent ranges.
-- The parent ranges remain as fallbacks for genuine schedule gaps; the runtime
-- gives these narrower descendant ranges precedence.

-- Art history: the schedule has clean chronological divisions before the
-- country-by-country portion begins.
INSERT OR IGNORE INTO lcc_range(start_code,end_code,concept_id) VALUES
  ('N5310',  'N5310.5', 1021),
  ('N5320',  'N5899',   1016),
  ('N5950',  'N6320',   1019),
  ('N6370',  'N6375',   1022),
  ('N6407',  'N6425',   1017),
  ('N6480',  'N6498',   1018);

-- The opening HD division is management, not an undifferentiated industry.
-- Reuse the existing Business Administration subject through polyhierarchy.
INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal)
VALUES(12272,5157,1);
INSERT OR IGNORE INTO lcc_range(start_code,end_code,concept_id)
VALUES('HD28','HD70',12272);
INSERT OR IGNORE INTO lcc_selector(concept_id,selector)
VALUES(12272,'HD28-70');

-- Science subclasses are themselves official top-level schedule divisions.
INSERT OR IGNORE INTO lcc_range(start_code,end_code,concept_id) VALUES
  ('QA1','QA939',   4916),
  ('QB1','QB991',   4780),
  ('QC1','QC999',   5030),
  ('QD1','QD999',   4803),
  ('QE1','QE996.5', 4904),
  ('QK1','QK989',   4797),
  ('QL1','QL991',   5048),
  ('QP1','QP981',   5043);
INSERT OR IGNORE INTO lcc_selector(concept_id,selector) VALUES
  (4916,'QA1-939'),
  (4780,'QB1-991'),
  (5030,'QC1-999'),
  (4803,'QD1-999'),
  (4803,'QD450-801'),
  (4904,'QE1-996.5'),
  (4797,'QK1-989'),
  (5048,'QL1-991'),
  (5043,'QP1-981');

-- Exact Open Library range-shaped notations previously overrode these
-- already-correct child ranges at their parents.
UPDATE lcc_selector SET concept_id=2728
WHERE concept_id=2725 AND selector='R131-R684';
UPDATE lcc_selector SET concept_id=2731
WHERE concept_id=2725 AND selector IN('R723-R723.7','R724-R726.2');

UPDATE lcc_selector SET concept_id=3607
WHERE concept_id=3319 AND selector='DG';
UPDATE lcc_selector SET concept_id=3666
WHERE concept_id=3319 AND selector='D203.2-D475';

-- Christianity's broad BR range contains well-defined theology, history and
-- biography divisions. Christian Biography already exists under its history
-- branch, so expose it here through polyhierarchy rather than duplicating it.
INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal)
VALUES(13206,4582,2);
INSERT OR IGNORE INTO lcc_range(start_code,end_code,concept_id) VALUES
  ('BR118',  'BR119.2', 4581),
  ('BR127',  'BR128',   4581),
  ('BR1600', 'BR1610.2',4588),
  ('BR1690', 'BR1725',  13206);

-- Commerce and business. Extend the Business range to the official start and
-- partition its major functional divisions among existing children.
UPDATE lcc_range SET start_code='HF4999.2'
WHERE concept_id=5086 AND start_code='HF5001' AND end_code='HF6182';
UPDATE lcc_selector SET concept_id=52715
WHERE concept_id=5083 AND selector='HF54.5-54.56';
INSERT OR IGNORE INTO lcc_selector(concept_id,selector)
VALUES(5086,'HF4999.2-6182');
INSERT OR IGNORE INTO lcc_range(start_code,end_code,concept_id) VALUES
  ('HF5410','HF5418.9',5189),
  ('HF5546','HF5548.6',5200),
  ('HF5551','HF5585',  5142),
  ('HF5601','HF5688',  5105);

-- Communication and mass media: add only the substantial official divisions.
INSERT INTO concept(concept_id,preferred_label) VALUES
  (53390,'Mass Media Research & Education'),
  (53391,'Oral Communication & Speech'),
  (53392,'Media Policy');
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal) VALUES
  (53390,4408,0),
  (53391,4408,0),
  (53392,4408,0);
INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal)
VALUES(52675,4408,1);
UPDATE lcc_range SET concept_id=53390
WHERE start_code='P91.3' AND end_code='P91.5' AND concept_id=4408;
UPDATE lcc_range SET concept_id=53391
WHERE start_code='P95' AND end_code='P95.6' AND concept_id=4408;
INSERT INTO lcc_range(start_code,end_code,concept_id) VALUES
  ('P94',  'P94.9',  52675),
  ('P95.8','P95.82', 53392);

-- Bare subclass selectors are not emitted as fallback keys for numbered LCC
-- calls.  Restore class-wide ranges removed under that incorrect assumption;
-- retain them until each schedule has a complete disjoint leaf partition.

INSERT INTO lcc_range(start_code,end_code,concept_id)
VALUES
  ('HD','HD',5104),
  ('HG','HG',5142),
  ('B','B',4487),
  ('BF','BF',5381),
  ('BH','BH',11215),
  ('BJ','BJ',4494),
  ('GR','GR',5079),
  ('HF','HF',5083),
  ('JA','JA',5369),
  ('GV','GV',4628),
  ('SK','SK',4628),
  ('T1','T995',5563),
  ('TA1','TA2040',5563),
  ('QA1','QA939',4916),
  ('QB1','QB991',4780),
  ('QC1','QC999',5030),
  ('QD1','QD999',4803),
  ('QE1','QE996.5',4904),
  ('QK1','QK989',4797),
  ('QL1','QL991',5048),
  ('QP1','QP981',5043),
  ('GE1','GE350',2377);

-- Partition the high-use QH1-QH278.5 natural-history schedule.  Consolidate
-- the duplicate Microscopy leaf into the richer Life Sciences hierarchy and
-- reuse that concept through polyhierarchy.

INSERT INTO concept(concept_id,preferred_label)
VALUES
  (53309,'Biological Classification & Nomenclature'),
  (53310,'Freshwater Biology');

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal)
VALUES
  (53309,4982,0),
  (53310,4982,0),
  (4981,5724,1);

UPDATE bisac_selector SET concept_id=4981 WHERE concept_id=4877;
DELETE FROM concept WHERE concept_id=4877;

DELETE FROM lcc_range WHERE concept_id=4982;

INSERT INTO lcc_range(start_code,end_code,concept_id)
VALUES
  ('QH1','QH72',4879),
  ('QH75','QH78',5013),
  ('QH83','QH83.5',53309),
  ('QH84','QH90.8',5024),
  ('QH96','QH100',53310),
  ('QH101','QH199.62',5024),
  ('QH251','QH255',8888);

DELETE FROM lcc_range
WHERE concept_id=4981
  AND (start_code,end_code) IN (
    ('QH201','QH278.5'),
    ('QH205','QH205.2')
  );

UPDATE lcc_range SET end_code='QH225'
WHERE concept_id=7745
  AND start_code='QH211'
  AND end_code='QH212';

INSERT INTO lcc_selector(concept_id,selector)
VALUES
  (5014,'QH81'),
  (4879,'QH82'),
  (4981,'QH201-QH278.5'),
  (4981,'QH201'),
  (4981,'QH202'),
  (4981,'QH203'),
  (4981,'QH205'),
  (4981,'QH205.2'),
  (8888,'QH206'),
  (4981,'QH261'),
  (4981,'QH277'),
  (4981,'QH278'),
  (4981,'QH278.5');

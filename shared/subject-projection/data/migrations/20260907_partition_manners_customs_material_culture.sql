BEGIN;

UPDATE lcc_selector SET concept_id=12310
WHERE concept_id=5068 AND selector IN
 ('GT165-GT165.5','GT170','GT171','GT172','GT175-GT195','GT201-GT384','GT420-GT476');

UPDATE lcc_selector SET concept_id=2629
WHERE concept_id=5068 AND selector='GT495';

UPDATE lcc_selector SET concept_id=676
WHERE concept_id=5068 AND selector IN
 ('GT500','GT503','GT505','GT507','GT513','GT518','GT520','GT529','GT529.2');

UPDATE lcc_selector SET concept_id=12313
WHERE concept_id=5068 AND selector IN ('GT3400','GT3408','GT3490');

UPDATE lcc_selector SET concept_id=12314
WHERE concept_id=5068 AND selector IN
 ('GT5220','GT5230-GT5260','GT5280-GT5285');

UPDATE lcc_selector SET concept_id=12315
WHERE concept_id=5068 AND selector IN ('GT5320','GT5350-GT5680');

UPDATE lcc_selector SET concept_id=12316
WHERE concept_id=5068 AND selector IN
 ('GT5750','GT5760-GT5790','GT5810-GT5905','GT5910-GT5960',
  'GT6010-GT6060','GT6110-GT6390');

-- These leaves previously carried exact textual spans only. They must own the
-- permissive intervals before the overlapping parent ranges are removed.
INSERT OR IGNORE INTO lcc_range(start_code,end_code,concept_id) VALUES
 ('GT500','GT2370',676),
 ('GT2400','GT3390.5',12312),
 ('GT3400','GT5090',12313);

DELETE FROM lcc_range WHERE concept_id=5068;

COMMIT;

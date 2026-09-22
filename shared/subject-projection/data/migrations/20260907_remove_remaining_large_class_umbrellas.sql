-- Remove broad discipline ranges that duplicate whole established subtrees.
-- Retain printed interval evidence exactly on each parent. The one coherent
-- historical sub-schedule is routed to its existing Modern History child.

DELETE FROM lcc_range
WHERE concept_id IN (1, 250, 969, 5244, 5369, 5381, 5532);

DELETE FROM lcc_range
WHERE concept_id = 3319
  AND NOT (start_code = 'D203.2' AND end_code = 'D475');

UPDATE lcc_range
SET concept_id = 3666
WHERE concept_id = 3319
  AND start_code = 'D203.2'
  AND end_code = 'D475';

INSERT OR IGNORE INTO lcc_selector(concept_id, selector) VALUES
  (1, 'T-TX'),
  (1, 'V-VM'),
  (250, 'TK1-TK9971'),
  (969, 'N1-N9211'),
  (5244, 'HQ1-HQ2044'),
  (5369, 'JA'),
  (5381, 'BF'),
  (5532, 'B1-B5802'),
  (3319, 'C-CT'),
  (3319, 'D-DX'),
  (3319, 'D1-D2027'),
  (3319, 'D1-DX301'),
  (3319, 'E-F'),
  (3319, 'D203.2-D475');

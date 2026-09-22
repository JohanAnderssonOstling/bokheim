-- These ranges are wholly contained by a broader range on the same subject.

DELETE FROM lcc_range
WHERE (concept_id,start_code,end_code) IN (
  (4801,'QK711','QK711.2'),
  (4801,'QK714','QK714.5'),
  (4801,'QK715','QK715.5'),
  (4800,'QK900.7','QK900.73')
);

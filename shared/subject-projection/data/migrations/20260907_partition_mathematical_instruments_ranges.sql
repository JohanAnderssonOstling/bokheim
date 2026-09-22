-- Resolve the nested QA71-QA99 printed divisions into mutually exclusive call
-- ranges. Preserve the official broad divisions as exact selectors.

DELETE FROM lcc_range WHERE concept_id IN (4949,7945);

INSERT INTO lcc_range(start_code,end_code,concept_id)
VALUES
  ('QA75','QA75.49',7945),
  ('QA91','QA99',7945);

INSERT OR IGNORE INTO lcc_selector(concept_id,selector)
VALUES
  (4949,'QA71'),
  (4949,'QA72'),
  (4949,'QA71-90'),
  (7945,'QA75-99');

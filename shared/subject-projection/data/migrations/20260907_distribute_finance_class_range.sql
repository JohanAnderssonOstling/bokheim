-- Replace the whole-HG Finance range with the major official schedules already
-- represented by existing children. Exact HG remains on Finance as fallback for
-- genuine gaps and explicitly broad classifications.

DELETE FROM lcc_range
WHERE concept_id = 5142
  AND start_code = 'HG'
  AND end_code = 'HG';

INSERT INTO lcc_range(start_code, end_code, concept_id) VALUES
  ('HG201',  'HG1496',   5192),
  ('HG3810', 'HG4000',   5147),
  ('HG4001', 'HG4285',  13006),
  ('HG4501', 'HG6052.Z', 5183),
  ('HG8011', 'HG9999',   5230);

-- Banking and Credit already own their official broad ranges:
--   HG1501-HG3550 -> Banking
--   HG3691-HG3769 -> Credit & Loans

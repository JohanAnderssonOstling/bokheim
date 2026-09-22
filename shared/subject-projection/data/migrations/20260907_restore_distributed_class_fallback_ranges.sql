-- Retain broad class ranges as parent fallbacks. Existing and newly added child
-- ranges still win because ancestor matches are shadowed by descendants. This
-- preserves coverage for legitimate schedule gaps without artificial leaves.

INSERT INTO lcc_range(start_code, end_code, concept_id) VALUES
  ('HG',     'HG',       5142),
  ('HF',     'HF',       5083),
  ('HD',     'HD',       5104),
  ('R',      'RZ',       2725),
  ('R129',   'R130.52',  2725),
  ('Q',      'QR',       4779),
  ('Q1',     'Q390',     4779),
  ('T',      'TX',          1),
  ('V',      'VM',          1),
  ('TK1',    'TK9971',    250),
  ('N1',     'N9211',     969),
  ('C',      'CT',       3319),
  ('D',      'DX',       3319),
  ('D1',     'D2027',    3319),
  ('D1',     'DX301',    3319),
  ('E',      'F',        3319),
  ('HQ1',    'HQ2044',   5244),
  ('JA',     'JA',       5369),
  ('BF',     'BF',       5381),
  ('B1',     'B5802',    5532);

-- Distribute class-wide HF and HD ranges to existing schedule subjects.
-- Exact class selectors remain available for genuinely broad or gap material.

DELETE FROM lcc_range
WHERE (concept_id = 5083 AND start_code = 'HF' AND end_code = 'HF')
   OR (concept_id = 5104 AND start_code = 'HD' AND end_code = 'HD');

INSERT INTO lcc_range(start_code, end_code, concept_id) VALUES
  ('HF1351', 'HF1647', 5087),
  ('HF1701', 'HF2701', 5090),
  ('HF3000', 'HF4055', 5087),
  ('HF5001', 'HF6182', 5086),
  ('HD28',   'HD9999', 5157);

INSERT OR IGNORE INTO lcc_selector(concept_id, selector) VALUES
  (5104, 'HD'),
  (5086, 'HF5001-6182');

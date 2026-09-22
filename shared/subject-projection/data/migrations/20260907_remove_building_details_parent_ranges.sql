-- Existing children cover the useful specific building-detail schedules. Keep
-- broad printed spans exact on the parent and remove permissive overlap.

DELETE FROM lcc_range
WHERE concept_id = 203;

INSERT OR IGNORE INTO lcc_selector(concept_id, selector) VALUES
  (203, 'TH2025-3000'),
  (203, 'TH2150-2160'),
  (203, 'TH2170-2170.5'),
  (203, 'TH2281-2288');

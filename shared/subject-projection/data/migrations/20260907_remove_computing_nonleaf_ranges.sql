-- The computing schedule is exhaustively represented by exact topic selectors
-- and dedicated leaf ranges. Keep printed divisions exact on their nonleafs;
-- do not retain permissive overlapping umbrellas.

DELETE FROM lcc_range
WHERE concept_id IN (62,152,116,11664);

INSERT OR IGNORE INTO lcc_selector(concept_id,selector)
VALUES
  (62,'QA75.5'),
  (62,'QA76'),
  (62,'QA75.5-76.95'),
  (62,'QA76.5-76.73'),
  (152,'QA76.6'),
  (152,'QA76.6-76.66'),
  (116,'QA76.7'),
  (116,'QA76.73'),
  (116,'QA76.7-76.73'),
  (11664,'QA76.75'),
  (11664,'QA76.765'),
  (11664,'QA76.75-76.765');

-- Once a nested schedule-wide range is partitioned among leaves, an incoming
-- printed interval is deliberately not matched by overlap. Preserve that broad
-- classification exactly on its nonleaf division while ordinary numbered calls
-- continue through the mutually exclusive child ranges.

INSERT OR IGNORE INTO lcc_selector(concept_id,selector)
VALUES
  (5046,'QP351-495'),
  (5044,'QP501-801'),
  (5045,'QP901-981'),
  (4804,'QD71-142'),
  (4806,'QD146-197'),
  (4807,'QD241-441'),
  (4808,'QD450-882'),
  (4805,'QD901-999'),
  (5033,'QC120-168.86'),
  (5033,'QC120-168.85'),
  (5032,'QC170-197'),
  (5031,'QC221-246');

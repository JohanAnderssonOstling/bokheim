-- Specific bridge topics already own the useful numbered spans. Preserve broad
-- printed intervals exactly on the parent instead of leaving permissive ranges.

DELETE FROM lcc_range
WHERE concept_id = 196;

INSERT OR IGNORE INTO lcc_selector(concept_id, selector) VALUES
  (196, 'TG1-4'),
  (196, 'TG1-470'),
  (196, 'TG139-140'),
  (196, 'TG144-160'),
  (196, 'TG145');

-- Metalworking's specific high-use divisions already have children. Preserve
-- broad printed classifications as exact selectors and keep TS205 (General
-- works) on the parent rather than creating a forbidden General leaf.

DELETE FROM lcc_range
WHERE concept_id = 353;

INSERT OR IGNORE INTO lcc_selector(concept_id, selector) VALUES
  (353, 'TS200-770'),
  (353, 'TS551-552'),
  (353, 'TS590-599'),
  (353, 'TS205');

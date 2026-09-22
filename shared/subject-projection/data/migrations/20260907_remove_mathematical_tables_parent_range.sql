-- The only uncovered valid heading is low-use QA51 Factor Tables, which does
-- not merit another leaf. Preserve it and the printed division exactly.

DELETE FROM lcc_range
WHERE concept_id=4970
  AND start_code='QA47'
  AND end_code='QA59';

INSERT OR IGNORE INTO lcc_selector(concept_id,selector)
VALUES
  (4970,'QA47-59'),
  (4970,'QA51');

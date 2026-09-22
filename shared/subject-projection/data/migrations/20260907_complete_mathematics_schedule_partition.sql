-- Every high-use QA heading and every occupied continuous division now has a
-- specific owner. Preserve only the printed whole schedule on Mathematics.

DELETE FROM lcc_range
WHERE concept_id=4916
  AND start_code='QA1'
  AND end_code='QA939';

INSERT OR IGNORE INTO lcc_selector(concept_id,selector)
VALUES (4916,'QA1-939');

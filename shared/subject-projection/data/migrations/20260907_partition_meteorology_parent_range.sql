-- Existing children form a complete contiguous QC851-QC999 partition.

DELETE FROM lcc_range
WHERE concept_id=5038
  AND start_code='QC851'
  AND end_code='QC999';

INSERT OR IGNORE INTO lcc_selector(concept_id,selector)
VALUES (5038,'QC851-999');

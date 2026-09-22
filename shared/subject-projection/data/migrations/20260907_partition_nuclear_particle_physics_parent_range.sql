-- The three existing leaves form a complete, contiguous QC770-QC798
-- partition. Preserve the printed whole division exactly on the parent.

DELETE FROM lcc_range
WHERE concept_id=5039
  AND start_code='QC770'
  AND end_code='QC798';

INSERT OR IGNORE INTO lcc_selector(concept_id,selector)
VALUES (5039,'QC770-798');

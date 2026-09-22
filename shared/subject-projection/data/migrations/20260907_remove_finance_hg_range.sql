-- Bare HG is already an exact Finance selector.  The same-endpoint range is
-- redundant and competes with Finance's ranged children.

DELETE FROM lcc_range
WHERE concept_id=5142
  AND start_code='HG'
  AND end_code='HG';

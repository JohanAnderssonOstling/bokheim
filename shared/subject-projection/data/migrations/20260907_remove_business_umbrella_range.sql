-- Business's two narrow parent ranges are reviewed uncovered gaps.  The full
-- HF4999.2-HF6182 umbrella overlaps virtually every ranged child and defeats
-- that partition, so remove only the umbrella.

DELETE FROM lcc_range
WHERE concept_id=5086
  AND start_code='HF4999.2'
  AND end_code='HF6182';

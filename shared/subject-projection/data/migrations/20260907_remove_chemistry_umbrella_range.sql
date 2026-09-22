-- Bare QD remains the exact general Chemistry fallback; detailed chemistry
-- fields already own their ranges.

DELETE FROM lcc_range
WHERE concept_id=4803
  AND start_code='QD1'
  AND end_code='QD999';

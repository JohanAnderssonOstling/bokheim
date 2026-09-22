-- Bare QC remains the exact fallback for general Physics.  Remove its
-- redundant schedule-wide range and give the two precise residual spans to
-- their existing leaf subjects.

UPDATE lcc_range SET concept_id=4885
WHERE concept_id=5030
  AND start_code='QC19.2'
  AND end_code='QC20.85';

UPDATE lcc_range SET concept_id=4834
WHERE concept_id=5030
  AND start_code='QC71.82'
  AND end_code='QC73.8';

DELETE FROM lcc_range
WHERE concept_id=5030
  AND start_code='QC1'
  AND end_code='QC999';

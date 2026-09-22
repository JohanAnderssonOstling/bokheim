-- TA is represented by the Civil, Structural & Construction class subject.
-- Specific schedules are already partitioned into exclusive leaves, so the
-- class-wide permissive range must not remain on Technology & Engineering.

DELETE FROM lcc_range
WHERE concept_id = 5563
  AND start_code = 'TA1'
  AND end_code = 'TA2040';

INSERT OR IGNORE INTO lcc_selector(concept_id, selector)
VALUES (5564, 'TA1-2040');

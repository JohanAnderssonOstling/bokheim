BEGIN;

-- Administration and era descendants already own the official E751-E837.7
-- sections. Keep only E740-E749 as direct broad twentieth-century material.
DELETE FROM lcc_range
WHERE concept_id=3405 AND class_letters='E'
  AND NOT (start_value='740' AND end_value='749');

COMMIT;

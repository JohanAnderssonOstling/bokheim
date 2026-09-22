BEGIN;

-- These ranges describe broad Midwestern regions in the LCC F schedule.
UPDATE lcc_range SET concept_id=3420
WHERE concept_id=3418 AND class_letters='F' AND (
  (start_value='350.3' AND end_value='358.2') OR
  (start_value='358' AND end_value='358.2') OR
  (start_value='550.5' AND end_value='556') OR
  (start_value='590.2' AND end_value='596') OR
  (start_value='596.2' AND end_value='596.3')
);

-- These ranges describe the Rocky Mountain, Southwest, and Pacific regions.
UPDATE lcc_range SET concept_id=3423
WHERE concept_id=3418 AND class_letters='F' AND (
  (start_value='785.15' AND end_value='790') OR
  (start_value='850.5' AND end_value='855.2') OR
  (start_value='851.72' AND end_value='854.2')
);

UPDATE source_selector SET concept_id=3423
WHERE concept_id=3418 AND system_id='lcc' AND selector='F722';

COMMIT;

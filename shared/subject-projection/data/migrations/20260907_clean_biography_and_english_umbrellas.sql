BEGIN;

-- Biography's five children cover every meaningful part of CT21–CT9999.
-- Keep the exact CT umbrella code but remove its overlapping permissive range.
DELETE FROM lcc_range WHERE concept_id=3216;

-- PE1074.5–PE1074.85 is part of the existing English-language history span.
UPDATE lcc_range SET concept_id=12196
WHERE concept_id=3709 AND class_letters='PE'
  AND start_value='1074.5' AND end_value='1074.85';

COMMIT;

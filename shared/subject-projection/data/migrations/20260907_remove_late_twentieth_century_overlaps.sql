BEGIN;

-- The three existing period children (1961-1969, 1969-1981, and 1981-2000)
-- own the complete E838-E889 schedule. Remove the older parent umbrella and
-- its redundant administration/topic ranges.
DELETE FROM lcc_range WHERE concept_id=3413 AND class_letters='E';

COMMIT;

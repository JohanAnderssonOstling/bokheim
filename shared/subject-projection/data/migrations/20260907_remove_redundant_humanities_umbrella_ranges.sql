BEGIN;
DELETE FROM lcc_range WHERE concept_id=4549 AND class_letters='BL' AND start_value='1' AND end_value='52.9';
DELETE FROM lcc_range WHERE concept_id=4582 AND class_letters='BR' AND NOT (start_value='1' AND end_value='1725');
DELETE FROM lcc_range WHERE concept_id=3749 AND class_letters='PD' AND NOT (start_value='1' AND end_value='7159');
DELETE FROM lcc_range WHERE concept_id=3798 AND class_letters='PJ' AND NOT (start_value='1' AND end_value='9500');
COMMIT;

BEGIN;

DELETE FROM lcc_range
WHERE concept_id=13535
  AND start_code='HE7521' AND end_code='HE7549';

COMMIT;

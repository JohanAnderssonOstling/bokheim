BEGIN;

DELETE FROM lcc_range
WHERE (concept_id=5301 AND start_code='KBU30.5' AND end_code='KBU321978a')
   OR (concept_id=5301 AND start_code='KBU34' AND end_code='KBU341978a')
   OR (concept_id=5562 AND start_code='KJV444.12' AND end_code='KJV4.44218e+07');

COMMIT;

BEGIN;

DELETE FROM lcc_range
WHERE (concept_id=255 AND start_code='TK4652.2' AND end_code='TK4600.5')
   OR (concept_id=3320 AND start_code='DT159.92865' AND end_code='DT159.92687')
   OR (concept_id=3333 AND start_code='DT2788' AND end_code='DT1795')
   OR (concept_id=5276 AND start_code='KDK975' AND end_code='KDK952')
   OR (concept_id=5308 AND start_code='KJC1542' AND end_code='KJC1541')
   OR (concept_id=10113 AND start_code='DS70.95' AND end_code='DS70.76')
   OR (concept_id=13308 AND start_code='KEO1053.2' AND end_code='KEO1043');

COMMIT;

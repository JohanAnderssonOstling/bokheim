-- Complete O section of the personal-bibliography schedule; P begins at
-- Z8650.5, so the O leaf ends at Z8650.
PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;

DELETE FROM source_selector WHERE concept_id=6313 AND system_id='lcc';
INSERT INTO source_selector VALUES(6313,'lcc','Z8639.4..Z8650');

COMMIT;

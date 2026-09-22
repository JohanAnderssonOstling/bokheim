-- Compact two further exact leaf schedules into their published spans.
PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;

DELETE FROM source_selector WHERE concept_id=12726 AND system_id='lcc';
INSERT INTO source_selector VALUES(12726,'lcc','HV6115..HV6190');

DELETE FROM source_selector WHERE concept_id=13324 AND system_id='lcc';
INSERT INTO source_selector VALUES(13324,'lcc','Q175.32.A..Q175.32.Z');

COMMIT;

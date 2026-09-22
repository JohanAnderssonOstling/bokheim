-- This terminal subject represents the complete physical-phenomena block of
-- spiritualism, from its heading through the A-Z special topics.
PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;

DELETE FROM source_selector WHERE concept_id=9546 AND system_id='lcc';
INSERT INTO source_selector VALUES(9546,'lcc','BF1371..BF1389.Z');

COMMIT;

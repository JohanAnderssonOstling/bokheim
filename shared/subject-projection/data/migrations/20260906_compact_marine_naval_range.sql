-- Marine & Naval is an intentionally broad leaf, so use the official complete
-- VM span instead of a sparse enumeration of representative subdivisions.
PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;

DELETE FROM source_selector WHERE concept_id=362 AND system_id='lcc';
INSERT INTO source_selector VALUES(362,'lcc','VM1..VM989');

COMMIT;

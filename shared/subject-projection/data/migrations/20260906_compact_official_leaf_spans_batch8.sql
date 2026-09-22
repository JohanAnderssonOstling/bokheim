-- Croatia local history is one official schedule span.
PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;
DELETE FROM source_selector WHERE concept_id=3546 AND system_id='lcc';
INSERT INTO source_selector VALUES(3546,'lcc','DR1502..DR1645');
COMMIT;

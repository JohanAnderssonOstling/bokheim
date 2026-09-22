-- Cuba is a leaf. Replace its enumerated general/history/province numbers with
-- the two contiguous country spans present in the LC schedule.
PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;

DELETE FROM source_selector WHERE concept_id=3380 AND system_id='lcc';
INSERT INTO source_selector VALUES(3380,'lcc','F1751..F1849.Z');
INSERT INTO source_selector VALUES(3380,'lcc','F1850..F1855.9.Z');

COMMIT;

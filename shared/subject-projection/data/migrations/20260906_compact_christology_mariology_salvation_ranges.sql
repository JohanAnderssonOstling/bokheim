-- The combined terminal subject maps exactly to three contiguous BT blocks:
-- Christology, Mariology (including associated cults), and Soteriology.
PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;

DELETE FROM source_selector WHERE concept_id=5797 AND system_id='lcc';
INSERT INTO source_selector VALUES(5797,'lcc','BT198..BT590');
INSERT INTO source_selector VALUES(5797,'lcc','BT595..BT694');
INSERT INTO source_selector VALUES(5797,'lcc','BT750..BT811');

COMMIT;

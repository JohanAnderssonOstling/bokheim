-- The leaf combines the LC 19th- and 20th-century drawing blocks.
PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;

DELETE FROM source_selector WHERE concept_id=8158 AND system_id='lcc';
INSERT INTO source_selector VALUES(8158,'lcc','NC89..NC95.5.Z');

COMMIT;

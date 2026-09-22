-- LC places the criticism-essay collections and their period subdivisions in
-- one continuous PR67-PR78 block.
PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;

DELETE FROM source_selector WHERE concept_id=7470 AND system_id='lcc';
INSERT INTO source_selector VALUES(7470,'lcc','PR67..PR78');

COMMIT;

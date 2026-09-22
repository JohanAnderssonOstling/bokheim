-- Collapse deliberately pruned official subtrees into their broad leaf ranges.
PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;

DELETE FROM source_selector WHERE concept_id=7770 AND system_id='lcc';
INSERT INTO source_selector VALUES(7770,'lcc','QK532.4..QK563.87');

-- Keep the war broad; detailed operations and biographies resolve to this leaf.
DELETE FROM source_selector WHERE concept_id=10109 AND system_id='lcc';
INSERT INTO source_selector VALUES(10109,'lcc','DF229..DF230.Z');

COMMIT;

-- Compact the BT83 modern schools-of-theology enumeration while reserving
-- BT83.6 exclusively for the existing Process Theology leaf.
PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;

CREATE TEMP TABLE _theology_base(id INTEGER NOT NULL);
INSERT INTO _theology_base SELECT COALESCE(MAX(concept_id),0) FROM concept;
INSERT INTO concept(concept_id,preferred_label)
SELECT id+1,'Modern Schools of Theology' FROM _theology_base;
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT id+1,4581,0 FROM _theology_base;
INSERT INTO source_selector(concept_id,system_id,selector)
SELECT id+1,'lcc','BT83..BT83.597' FROM _theology_base
UNION ALL SELECT id+1,'lcc','BT83.63..BT83.9' FROM _theology_base;

INSERT OR IGNORE INTO source_selector VALUES(12696,'lcc','BT83.6');
DELETE FROM source_selector WHERE concept_id=4581 AND system_id='lcc'
 AND (selector='BT83' OR selector GLOB 'BT83.*');

DROP TABLE _theology_base;
COMMIT;

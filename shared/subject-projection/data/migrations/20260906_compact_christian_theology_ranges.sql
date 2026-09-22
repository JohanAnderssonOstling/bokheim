-- Compact the BT doctrinal schedule into coherent terminal children and remove
-- enumerated/overlapping selectors from the Christian Theology parent.
PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;

CREATE TEMP TABLE _bt_base(id INTEGER NOT NULL);
INSERT INTO _bt_base SELECT COALESCE(MAX(concept_id),0) FROM concept;
CREATE TEMP TABLE _bt_node(seq INTEGER PRIMARY KEY,label TEXT,selector_a TEXT,selector_b TEXT);
INSERT INTO _bt_node VALUES
 (1,'General & Reference','BT1..BT18.Z','BT38..BT82.7'),
 (2,'Authority & Sources of Doctrine','BT88..BT92',NULL);
INSERT INTO concept(concept_id,preferred_label)
SELECT b.id+n.seq,n.label FROM _bt_node n CROSS JOIN _bt_base b;
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT b.id+n.seq,4581,
       COALESCE((SELECT MAX(ordinal)+1 FROM concept_parent WHERE parent_concept_id=4581),0)+n.seq-1
FROM _bt_node n CROSS JOIN _bt_base b;
INSERT INTO source_selector(concept_id,system_id,selector)
SELECT b.id+n.seq,'lcc',n.selector_a FROM _bt_node n CROSS JOIN _bt_base b;
INSERT INTO source_selector(concept_id,system_id,selector)
SELECT b.id+n.seq,'lcc',n.selector_b FROM _bt_node n CROSS JOIN _bt_base b
WHERE n.selector_b IS NOT NULL;

-- Doctrine/dogma, divine law and miracles, and creeds/catechisms.
DELETE FROM source_selector WHERE concept_id=5800 AND system_id='lcc';
INSERT INTO source_selector VALUES(5800,'lcc','BT19..BT37');
INSERT INTO source_selector VALUES(5800,'lcc','BT95..BT97.3');
INSERT INTO source_selector VALUES(5800,'lcc','BT990..BT1040');

INSERT OR IGNORE INTO source_selector VALUES(13767,'lcc','BT84..BT85');

DELETE FROM source_selector
WHERE concept_id=5797 AND system_id='lcc' AND selector IN ('BT675','BT680');
INSERT OR IGNORE INTO source_selector VALUES(5797,'lcc','BT675..BT694');

DELETE FROM source_selector WHERE concept_id=5798 AND system_id='lcc';
INSERT INTO source_selector VALUES(5798,'lcc','BT819..BT891.2');
INSERT INTO source_selector VALUES(5798,'lcc','BT899..BT940.2');
INSERT INTO source_selector VALUES(5798,'lcc','BT960..BT985');

DELETE FROM source_selector WHERE concept_id=4581 AND system_id='lcc';
INSERT INTO source_selector VALUES(4581,'lcc','BT');
INSERT INTO source_selector VALUES(4581,'lcc','BT94');

DROP TABLE _bt_node;
DROP TABLE _bt_base;
COMMIT;

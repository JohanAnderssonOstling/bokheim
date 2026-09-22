-- Restrict Northern Europe to the DL general schedule and remove duplicated
-- national schedules now owned by country descendants.
PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;

INSERT OR IGNORE INTO source_selector VALUES(2465,'lcc','DL7..DL11.5');

UPDATE concept_parent SET parent_concept_id=3609 WHERE parent_concept_id=11440;
DELETE FROM concept WHERE concept_id=11440;

DELETE FROM source_selector WHERE concept_id=3609 AND system_id='lcc';
INSERT INTO source_selector VALUES(3609,'lcc','DL');

CREATE TEMP TABLE _north_base(id INTEGER NOT NULL);
INSERT INTO _north_base SELECT COALESCE(MAX(concept_id),0) FROM concept;
CREATE TEMP TABLE _north_node(seq INTEGER PRIMARY KEY,label TEXT,selector_a TEXT,selector_b TEXT);
INSERT INTO _north_node VALUES
 (1,'General & Reference','DL1..DL6.8',NULL),
 (2,'Antiquities','DL20..DL23',NULL),
 (3,'Culture & Ethnography','DL41..DL42.Z',NULL),
 (4,'General Historical Studies','DL43..DL56.Z','DL58..DL60.Z');
INSERT INTO concept(concept_id,preferred_label)
SELECT b.id+n.seq,n.label FROM _north_node n CROSS JOIN _north_base b;
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT b.id+n.seq,3609,
       COALESCE((SELECT MAX(ordinal)+1 FROM concept_parent WHERE parent_concept_id=3609),0)+n.seq
FROM _north_node n CROSS JOIN _north_base b;
INSERT INTO source_selector(concept_id,system_id,selector)
SELECT b.id+n.seq,'lcc',n.selector_a FROM _north_node n CROSS JOIN _north_base b;
INSERT INTO source_selector(concept_id,system_id,selector)
SELECT b.id+n.seq,'lcc',n.selector_b FROM _north_node n CROSS JOIN _north_base b WHERE n.selector_b IS NOT NULL;

DROP TABLE _north_node;
DROP TABLE _north_base;
COMMIT;

-- Compact Portugal into broad chronology and geography leaves; move its
-- description-and-travel block to the existing Spain & Portugal travel leaf.
PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;

INSERT OR IGNORE INTO source_selector VALUES(2471,'lcc','DP521..DP526.5');

DELETE FROM source_selector
WHERE system_id='lcc' AND concept_id IN (10328,3585,10117,11578,11579,11580);
INSERT INTO source_selector VALUES
 (10328,'lcc','DP558..DP567.Z'),
 (3585,'lcc','DP568..DP618'),
 (10117,'lcc','DP620..DP629.Z'),
 (11578,'lcc','DP632..DP644.9'),
 (11579,'lcc','DP645..DP669'),
 (11580,'lcc','DP670..DP682.5');

CREATE TEMP TABLE _portugal_base(id INTEGER NOT NULL);
INSERT INTO _portugal_base SELECT COALESCE(MAX(concept_id),0) FROM concept;
CREATE TEMP TABLE _portugal_node(seq INTEGER PRIMARY KEY,label TEXT,selector_a TEXT,selector_b TEXT);
INSERT INTO _portugal_node VALUES
 (1,'General & Reference','DP501..DP520.5',NULL),
 (2,'Culture & General Historical Studies','DP528..DP557',NULL),
 (3,'Lisbon','DP752..DP776',NULL),
 (4,'Other Regions & Cities','DP702..DP751.Z','DP777..DP802'),
 (5,'Portuguese Colonies','DP900..DP900.22',NULL);
INSERT INTO concept(concept_id,preferred_label)
SELECT b.id+n.seq,n.label FROM _portugal_node n CROSS JOIN _portugal_base b;
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT b.id+n.seq,3611,
       COALESCE((SELECT MAX(ordinal)+1 FROM concept_parent WHERE parent_concept_id=3611),0)+n.seq-1
FROM _portugal_node n CROSS JOIN _portugal_base b;
INSERT INTO source_selector(concept_id,system_id,selector)
SELECT b.id+n.seq,'lcc',n.selector_a FROM _portugal_node n CROSS JOIN _portugal_base b;
INSERT INTO source_selector(concept_id,system_id,selector)
SELECT b.id+n.seq,'lcc',n.selector_b FROM _portugal_node n CROSS JOIN _portugal_base b
WHERE n.selector_b IS NOT NULL;

DELETE FROM source_selector WHERE concept_id=3611 AND system_id='lcc';

DROP TABLE _portugal_node;
DROP TABLE _portugal_base;
COMMIT;

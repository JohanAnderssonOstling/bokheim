-- Complete HE-class ownership through existing modal branches and missing leaves.
PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;

CREATE TEMP TABLE _tr_base(id INTEGER NOT NULL);
INSERT INTO _tr_base SELECT COALESCE(MAX(concept_id),0) FROM concept;
CREATE TEMP TABLE _tr_node(seq INTEGER PRIMARY KEY,parent_id INTEGER NOT NULL,label TEXT NOT NULL,selector TEXT NOT NULL);
INSERT INTO _tr_node VALUES
 (1,488,'Transportation & Communications—General','HE1..HE198'),
 (2,488,'Transportation by Region or Country','HE201..HE302.2'),
 (3,5952,'Rural Transportation','HE315..HE316'),
 (4,5952,'Bicycle Transportation','HE5736..HE5739'),
 (5,5954,'Freight Forwarding','HE5999.A..HE5999.Z'),
 (6,5954,'Pneumatic Service','HE7511..HE7549'),
 (7,5955,'Electronic Mail Services','HE7551..HE7553'),
 (8,5955,'Online Information Services Industry','HE7581..HE7583'),
 (9,5952,'Commercial Cycling Businesses','HE9745');
INSERT INTO concept(concept_id,preferred_label)
SELECT b.id+n.seq,n.label FROM _tr_node n CROSS JOIN _tr_base b;
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT b.id+n.seq,n.parent_id,0 FROM _tr_node n CROSS JOIN _tr_base b;
INSERT INTO source_selector(concept_id,system_id,selector)
SELECT b.id+n.seq,'lcc',n.selector FROM _tr_node n CROSS JOIN _tr_base b;

DELETE FROM source_selector WHERE concept_id=488 AND system_id='lcc';
DELETE FROM source_selector WHERE concept_id=5954 AND system_id='lcc' AND selector IN ('HE7511','HE7513','HE7516');

DROP TABLE _tr_node;
DROP TABLE _tr_base;
COMMIT;

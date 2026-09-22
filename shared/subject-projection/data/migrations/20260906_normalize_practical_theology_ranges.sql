-- Complete the official BV partition with broad residual leaves, then remove
-- the Practical Theology parent enumeration.
PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;

CREATE TEMP TABLE _practical_base(id INTEGER NOT NULL);
INSERT INTO _practical_base SELECT COALESCE(MAX(concept_id),0) FROM concept;
CREATE TEMP TABLE _practical_node(seq INTEGER PRIMARY KEY,parent_id INTEGER,label TEXT,selector TEXT);
INSERT INTO _practical_node VALUES
 (1,4619,'Practical Theology—General','BV1..BV4'),
 (2,11588,'Christian Life—General & Religious Duties','BV4485..BV4525.9'),
 (3,11588,'Christian Life for Special Groups & Topics','BV4580..BV4599');
INSERT INTO concept(concept_id,preferred_label)
SELECT b.id+n.seq,n.label FROM _practical_node n CROSS JOIN _practical_base b;
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT b.id+n.seq,n.parent_id,n.seq FROM _practical_node n CROSS JOIN _practical_base b;
INSERT INTO source_selector(concept_id,system_id,selector)
SELECT b.id+n.seq,'lcc',n.selector FROM _practical_node n CROSS JOIN _practical_base b;

DELETE FROM source_selector
WHERE concept_id=4619 AND system_id='lcc' AND selector<>'BV';

DROP TABLE _practical_node;
DROP TABLE _practical_base;
COMMIT;

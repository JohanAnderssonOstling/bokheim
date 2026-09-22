-- Remove overlapping England umbrellas and give each LC span one terminal
-- owner. Description/travel belongs in the travel taxonomy; London and the
-- historic-places block remain under England history as leaves.
PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;

DELETE FROM source_selector WHERE concept_id=3566 AND system_id='lcc';

DELETE FROM source_selector WHERE concept_id=10093 AND system_id='lcc';
INSERT INTO source_selector VALUES(10093,'lcc','DA129..DA162');

INSERT OR IGNORE INTO source_selector VALUES(4175,'lcc','DA605..DA632');

CREATE TEMP TABLE _eng_base(id INTEGER NOT NULL);
INSERT INTO _eng_base SELECT COALESCE(MAX(concept_id),0) FROM concept;
CREATE TEMP TABLE _eng_node(seq INTEGER PRIMARY KEY,label TEXT,selector TEXT);
INSERT INTO _eng_node VALUES
 (1,'Historic Places & Monuments','DA660..DA669'),
 (2,'London','DA675..DA689');
INSERT INTO concept(concept_id,preferred_label)
SELECT b.id+n.seq,n.label FROM _eng_node n CROSS JOIN _eng_base b;
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT b.id+n.seq,3566,
       COALESCE((SELECT MAX(ordinal)+1 FROM concept_parent WHERE parent_concept_id=3566),0)+n.seq-1
FROM _eng_node n CROSS JOIN _eng_base b;
INSERT INTO source_selector(concept_id,system_id,selector)
SELECT b.id+n.seq,'lcc',n.selector FROM _eng_node n CROSS JOIN _eng_base b;

DROP TABLE _eng_node;
DROP TABLE _eng_base;
COMMIT;

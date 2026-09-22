-- Replace the permissive all-Latin-America F range on a nonleaf with
-- mutually exclusive country/region leaves from the LC schedule.
PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;

INSERT OR IGNORE INTO source_selector VALUES(3382,'lcc','F1912..F1930.Z');
INSERT OR IGNORE INTO source_selector VALUES(3381,'lcc','F1931..F1941.Z');

CREATE TEMP TABLE _la_base(id INTEGER NOT NULL);
INSERT INTO _la_base SELECT COALESCE(MAX(concept_id),0) FROM concept;
CREATE TEMP TABLE _la_node(seq INTEGER PRIMARY KEY,label TEXT,selector TEXT);
INSERT INTO _la_node VALUES
 (1,'Puerto Rico','F1951..F1983.Z'),
 (2,'Lesser Antilles & Caribbean','F2001..F2191.Z');
INSERT INTO concept(concept_id,preferred_label)
SELECT b.id+n.seq,n.label FROM _la_node n CROSS JOIN _la_base b;
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT b.id+n.seq,3385,
       COALESCE((SELECT MAX(ordinal)+1 FROM concept_parent WHERE parent_concept_id=3385),0)+n.seq-1
FROM _la_node n CROSS JOIN _la_base b;
INSERT INTO source_selector(concept_id,system_id,selector)
SELECT b.id+n.seq,'lcc',n.selector FROM _la_node n CROSS JOIN _la_base b;

-- F975 is the exact Latin-America-wide schedule entry. Everything else on
-- this node is either the old umbrella or an enumeration already contained
-- by one of its country children.
DELETE FROM source_selector
WHERE concept_id=3385 AND system_id='lcc' AND selector<>'F975';

DROP TABLE _la_node;
DROP TABLE _la_base;
COMMIT;

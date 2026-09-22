-- Normalize Albania to mutually exclusive broad DR ranges.
PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;

DELETE FROM source_selector WHERE system_id='lcc' AND concept_id IN
 (3544,10466,11559,10224,10178,10320);
INSERT INTO source_selector VALUES
 (10224,'lcc','DR954..DR960.5'),
 (10178,'lcc','DR961..DR969'),
 (11559,'lcc','DR969.8..DR975'),
 (10466,'lcc','DR976..DR977.25'),
 (10320,'lcc','DR977.9..DR978.52');

CREATE TEMP TABLE _albania_base(id INTEGER NOT NULL);
INSERT INTO _albania_base SELECT COALESCE(MAX(concept_id),0) FROM concept;
CREATE TEMP TABLE _albania_node(seq INTEGER PRIMARY KEY,parent_id INTEGER,label TEXT,selector_a TEXT,selector_b TEXT);
INSERT INTO _albania_node VALUES
 (1,3544,'General & Reference','DR901..DR908.9','DR910..DR914.5'),
 (2,3544,'Culture & Ethnography','DR921..DR926',NULL),
 (3,3544,'General Historical Studies','DR926.92..DR953.Z',NULL),
 (4,3544,'Regions','DR996.A..DR996.Z',NULL),
 (5,3544,'Tirana','DR997',NULL),
 (6,3544,'Other Cities & Towns','DR998.A..DR998.Z',NULL),
 (7,2455,'Albania','DR909','DR915..DR918');
INSERT INTO concept(concept_id,preferred_label)
SELECT b.id+n.seq,n.label FROM _albania_node n CROSS JOIN _albania_base b;
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT b.id+n.seq,n.parent_id,
       COALESCE((SELECT MAX(ordinal)+1 FROM concept_parent WHERE parent_concept_id=n.parent_id),0)+n.seq
FROM _albania_node n CROSS JOIN _albania_base b;
INSERT INTO source_selector(concept_id,system_id,selector)
SELECT b.id+n.seq,'lcc',n.selector_a FROM _albania_node n CROSS JOIN _albania_base b;
INSERT INTO source_selector(concept_id,system_id,selector)
SELECT b.id+n.seq,'lcc',n.selector_b FROM _albania_node n CROSS JOIN _albania_base b WHERE n.selector_b IS NOT NULL;

DROP TABLE _albania_node;
DROP TABLE _albania_base;
COMMIT;

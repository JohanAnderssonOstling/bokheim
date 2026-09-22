-- Normalize Slovakia to mutually exclusive DB schedule ranges.
PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;

DELETE FROM source_selector WHERE system_id='lcc' AND concept_id IN
 (3540,10423,10551,10428,10436,10357,10615);
INSERT INTO source_selector VALUES
 (10357,'lcc','DB2795..DB2801'),
 (10423,'lcc','DB2805..DB2814.Z'),
 (10615,'lcc','DB2815..DB2822'),
 (10436,'lcc','DB2825..DB2831'),
 (10428,'lcc','DB2835..DB2842'),
 (10551,'lcc','DB2844');

CREATE TEMP TABLE _slovakia_base(id INTEGER NOT NULL);
INSERT INTO _slovakia_base SELECT COALESCE(MAX(concept_id),0) FROM concept;
CREATE TEMP TABLE _slovakia_node(seq INTEGER PRIMARY KEY,parent_id INTEGER,label TEXT,selector TEXT);
INSERT INTO _slovakia_node VALUES
 (1,3540,'General & Reference','DB2700..DB2717'),
 (2,3540,'Culture & Ethnography','DB2730..DB2743'),
 (3,3540,'General Historical Studies','DB2743.9..DB2784.Z'),
 (4,3540,'Early & Medieval Slovakia, to 1800','DB2785..DB2791'),
 (5,3540,'Independent Slovakia, 1993–Present','DB2845..DB2850.Z'),
 (6,3540,'Regions & Cities','DB3000..DB3150'),
 (7,2455,'Slovakia','DB2718..DB2722');
INSERT INTO concept(concept_id,preferred_label)
SELECT b.id+n.seq,n.label FROM _slovakia_node n CROSS JOIN _slovakia_base b;
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT b.id+n.seq,n.parent_id,
       COALESCE((SELECT MAX(ordinal)+1 FROM concept_parent WHERE parent_concept_id=n.parent_id),0)+n.seq
FROM _slovakia_node n CROSS JOIN _slovakia_base b;
INSERT INTO source_selector(concept_id,system_id,selector)
SELECT b.id+n.seq,'lcc',n.selector FROM _slovakia_node n CROSS JOIN _slovakia_base b;

DROP TABLE _slovakia_node;
DROP TABLE _slovakia_base;
COMMIT;

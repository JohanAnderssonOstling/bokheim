-- Compact AG reference-work enumeration into official leaf spans.
PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;

DELETE FROM source_selector WHERE concept_id=4768 AND system_id='lcc';
INSERT INTO source_selector VALUES(4768,'lcc','AG195..AG196');

CREATE TEMP TABLE _ag_base(id INTEGER NOT NULL);
INSERT INTO _ag_base SELECT COALESCE(MAX(concept_id),0) FROM concept;
CREATE TEMP TABLE _ag_node(seq INTEGER PRIMARY KEY,label TEXT NOT NULL,selector TEXT NOT NULL);
INSERT INTO _ag_node VALUES
 (1,'General Dictionaries & Minor Encyclopedias','AG4..AG90'),
 (2,'General Reference Works','AG103..AG190'),
 (3,'Clipping Bureaus','AG500..AG551');
INSERT INTO concept(concept_id,preferred_label)
SELECT b.id+n.seq,n.label FROM _ag_node n CROSS JOIN _ag_base b;
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT b.id+n.seq,5604,0 FROM _ag_node n CROSS JOIN _ag_base b;
INSERT INTO source_selector(concept_id,system_id,selector)
SELECT b.id+n.seq,'lcc',n.selector FROM _ag_node n CROSS JOIN _ag_base b;

DELETE FROM source_selector WHERE concept_id=5604 AND system_id='lcc'
 AND selector NOT IN ('AG','AG1','AG2','AG3');

DROP TABLE _ag_node;
DROP TABLE _ag_base;
COMMIT;

-- Normalize Iceland after removing its national umbrella from Northern Europe.
PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;

DELETE FROM source_selector WHERE system_id='lcc' AND concept_id IN
 (3582,11523,11520,11524,11522,11521);
INSERT INTO source_selector VALUES
 (11523,'lcc','DL352..DL353'),
 (11524,'lcc','DL351'),
 (11524,'lcc','DL354..DL356.Z'),
 (11520,'lcc','DL357..DL360'),
 (11521,'lcc','DL365..DL373'),
 (11522,'lcc','DL375..DL380');
UPDATE concept SET preferred_label='Other Early & Medieval Iceland, to 1540' WHERE concept_id=11524;

CREATE TEMP TABLE _iceland_base(id INTEGER NOT NULL);
INSERT INTO _iceland_base SELECT COALESCE(MAX(concept_id),0) FROM concept;
CREATE TEMP TABLE _iceland_node(seq INTEGER PRIMARY KEY,parent_id INTEGER,label TEXT,selector TEXT);
INSERT INTO _iceland_node VALUES
 (1,3582,'General & Reference','DL301..DL308.Z'),
 (2,3582,'Culture & Ethnography','DL321..DL334.5'),
 (3,3582,'General Historical Studies','DL335..DL350.Z'),
 (4,3582,'Regions','DL396.A..DL396.Z'),
 (5,3582,'Cities & Towns','DL398.A..DL398.Z'),
 (6,2465,'Iceland','DL309..DL315');
INSERT INTO concept(concept_id,preferred_label)
SELECT b.id+n.seq,n.label FROM _iceland_node n CROSS JOIN _iceland_base b;
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT b.id+n.seq,n.parent_id,
       COALESCE((SELECT MAX(ordinal)+1 FROM concept_parent WHERE parent_concept_id=n.parent_id),0)+n.seq
FROM _iceland_node n CROSS JOIN _iceland_base b;
INSERT INTO source_selector(concept_id,system_id,selector)
SELECT b.id+n.seq,'lcc',n.selector FROM _iceland_node n CROSS JOIN _iceland_base b;

DROP TABLE _iceland_node;
DROP TABLE _iceland_base;
COMMIT;

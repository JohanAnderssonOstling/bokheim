-- DT541 and DT545 are complete country schedules, not generic French West
-- Africa selectors. Split them into broad country leaves.
PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;

CREATE TEMP TABLE _west_base(id INTEGER NOT NULL);
INSERT INTO _west_base SELECT COALESCE(MAX(concept_id),0) FROM concept;
CREATE TEMP TABLE _west_node(seq INTEGER PRIMARY KEY,label TEXT,selector TEXT);
INSERT INTO _west_node VALUES
 (1,'Benin','DT541..DT541.9.Z'),
 (2,'Côte d’Ivoire','DT545..DT545.9.Z');
INSERT INTO concept(concept_id,preferred_label)
SELECT b.id+n.seq,n.label FROM _west_node n CROSS JOIN _west_base b;
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT b.id+n.seq,10633,n.seq FROM _west_node n CROSS JOIN _west_base b;
INSERT INTO source_selector(concept_id,system_id,selector)
SELECT b.id+n.seq,'lcc',n.selector FROM _west_node n CROSS JOIN _west_base b;

DELETE FROM source_selector WHERE concept_id=10633 AND system_id='lcc'
 AND (selector GLOB 'DT541*' OR selector GLOB 'DT545*');

DROP TABLE _west_node;
DROP TABLE _west_base;
COMMIT;

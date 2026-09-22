-- Compact official Belgian local-history cutters and Cuban population cutters
-- into permissive leaf ranges.
PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;

CREATE TEMP TABLE _bc_base(id INTEGER NOT NULL);
INSERT INTO _bc_base SELECT COALESCE(MAX(concept_id),0) FROM concept;
CREATE TEMP TABLE _bc_node(seq INTEGER PRIMARY KEY,parent_id INTEGER,label TEXT,selector TEXT);
INSERT INTO _bc_node VALUES
 (1,3577,'Provinces & Regions','DH801.A..DH801.Z'),
 (2,3577,'Brussels','DH802..DH809.811'),
 (3,3577,'Other Cities & Towns','DH811.A..DH811.Z'),
 (4,3380,'Population Groups','F1789.A1..F1789.Z');
INSERT INTO concept(concept_id,preferred_label)
SELECT b.id+n.seq,n.label FROM _bc_node n CROSS JOIN _bc_base b;
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT b.id+n.seq,n.parent_id,n.seq FROM _bc_node n CROSS JOIN _bc_base b;
INSERT INTO source_selector(concept_id,system_id,selector)
SELECT b.id+n.seq,'lcc',n.selector FROM _bc_node n CROSS JOIN _bc_base b;

DELETE FROM source_selector WHERE concept_id=3577 AND system_id='lcc'
 AND (selector GLOB 'DH801.*' OR selector GLOB 'DH811.*'
      OR selector IN ('DH802..DH809.811','DH804.3..DH804.5','DH809..DH809.5','DH809.7..DH809.8','DH809.95'));
DELETE FROM source_selector WHERE concept_id=3380 AND system_id='lcc'
 AND selector GLOB 'F1789.*';

DROP TABLE _bc_node;
DROP TABLE _bc_base;
COMMIT;

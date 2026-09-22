-- Replace the enumerated American prose selector set with the six official
-- first-level LCC subdivisions of PS360-PS380.
PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

CREATE TEMP TABLE _prose_active(ok INTEGER PRIMARY KEY);
INSERT INTO _prose_active
SELECT 1 WHERE NOT EXISTS (
  SELECT 1 FROM concept_parent WHERE parent_concept_id=3860
);
CREATE TEMP TABLE _prose_base(id INTEGER NOT NULL);
INSERT INTO _prose_base SELECT COALESCE(MAX(concept_id),0) FROM concept;
CREATE TEMP TABLE _prose_node(seq INTEGER PRIMARY KEY,label TEXT NOT NULL,ordinal INTEGER NOT NULL,selector TEXT NOT NULL);
INSERT INTO _prose_node VALUES
 (1,'Periodicals, Societies & Serials',0,'PS360'),
 (2,'Treatises',1,'PS362'),
 (3,'Addresses, Essays & Lectures',2,'PS364'),
 (4,'Special Topics',3,'PS366.A..PS366.Z@3'),
 (5,'By Period',4,'PS367..PS369.2@3'),
 (6,'Prose Fiction',5,'PS370..PS380@3');

INSERT INTO concept(concept_id,preferred_label)
SELECT b.id+n.seq,n.label FROM _prose_node n CROSS JOIN _prose_base b JOIN _prose_active;
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal,display_order)
SELECT b.id+n.seq,3860,n.ordinal,n.ordinal FROM _prose_node n CROSS JOIN _prose_base b JOIN _prose_active;
INSERT INTO source_selector(concept_id,system_id,selector)
SELECT b.id+n.seq,'lcc',n.selector FROM _prose_node n CROSS JOIN _prose_base b JOIN _prose_active;

DELETE FROM source_selector
WHERE concept_id=3860 AND system_id='lcc' AND EXISTS (SELECT 1 FROM _prose_active);

DROP TABLE _prose_node;
DROP TABLE _prose_base;
DROP TABLE _prose_active;
COMMIT;

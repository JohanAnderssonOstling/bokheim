-- Replace repeated official A-Z cutter enumerations with ranged leaves.
PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;

CREATE TEMP TABLE _cut_base(id INTEGER NOT NULL);
INSERT INTO _cut_base SELECT COALESCE(MAX(concept_id),0) FROM concept;
CREATE TEMP TABLE _cut_node(seq INTEGER PRIMARY KEY,parent_id INTEGER,label TEXT,selector TEXT,old_parent INTEGER,stem TEXT);
INSERT INTO _cut_node VALUES
 (1,4370,'Other Special Topics in Authorship','PN171.A..PN171.Z',4370,'PN171.'),
 (2,4382,'Special Topics in Poetry','PN1059.A..PN1059.Z',4382,'PN1059.'),
 (3,4457,'Special Subjects in Roman Literature','PA6029.A..PA6029.Z',4457,'PA6029.'),
 (4,2842,'Special Types of Hospitals','RA975.A..RA975.Z',2842,'RA975.'),
 (5,2324,'Special School Facilities','LB3325.A..LB3325.Z',2315,'LB3325.');
INSERT INTO concept(concept_id,preferred_label)
SELECT b.id+n.seq,n.label FROM _cut_node n CROSS JOIN _cut_base b;
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT b.id+n.seq,n.parent_id,n.seq FROM _cut_node n CROSS JOIN _cut_base b;
INSERT INTO source_selector(concept_id,system_id,selector)
SELECT b.id+n.seq,'lcc',n.selector FROM _cut_node n CROSS JOIN _cut_base b;

DELETE FROM source_selector
WHERE system_id='lcc' AND EXISTS (
 SELECT 1 FROM _cut_node n
 WHERE n.old_parent=source_selector.concept_id
   AND source_selector.selector GLOB n.stem || '*'
   AND source_selector.selector NOT LIKE '%..%'
);

DROP TABLE _cut_node;
DROP TABLE _cut_base;
COMMIT;

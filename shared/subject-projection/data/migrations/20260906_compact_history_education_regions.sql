-- Replace enumerated History of Education country codes with disjoint regional leaves.
PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;

CREATE TEMP TABLE _edu_base(id INTEGER NOT NULL);
INSERT INTO _edu_base SELECT COALESCE(MAX(concept_id),0) FROM concept;
CREATE TEMP TABLE _edu_node(seq INTEGER PRIMARY KEY,label TEXT NOT NULL);
INSERT INTO _edu_node VALUES
 (1,'Central America & Caribbean'),(2,'Other South America'),(3,'Other Europe'),
 (4,'Other Asia'),(5,'Africa'),(6,'Oceania & Arctic Regions');
INSERT INTO concept(concept_id,preferred_label)
SELECT b.id+n.seq,n.label FROM _edu_node n CROSS JOIN _edu_base b;
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT b.id+n.seq,2207,0 FROM _edu_node n CROSS JOIN _edu_base b;

CREATE TEMP TABLE _edu_range(seq INTEGER NOT NULL,selector TEXT NOT NULL);
INSERT INTO _edu_range VALUES
 (1,'LA435..LA539'),
 (2,'LA540..LA554.Z'),(2,'LA560..LA609'),
 (3,'LA620..LA629.Z'),(3,'LA670..LA689.Z'),(3,'LA719..LA719.Z'),
 (3,'LA779.3..LA789.Z'),(3,'LA799.6..LA839.Z'),(3,'LA844.6..LA909.Z'),
 (3,'LA919.6..LA1040'),
 (4,'LA1050..LA1129.Z'),(4,'LA1135..LA1149.Z'),(4,'LA1155..LA1309.Z'),
 (4,'LA1320..LA1329.Z'),(4,'LA1340..LA1484'),
 (5,'LA1500..LA2090'),
 (6,'LA2100..LA2284');
INSERT INTO source_selector(concept_id,system_id,selector)
SELECT b.id+r.seq,'lcc',r.selector FROM _edu_range r CROSS JOIN _edu_base b;

DELETE FROM source_selector WHERE concept_id=2207 AND system_id='lcc';

DROP TABLE _edu_range;
DROP TABLE _edu_node;
DROP TABLE _edu_base;
COMMIT;

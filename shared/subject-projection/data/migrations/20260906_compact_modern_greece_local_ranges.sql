-- Compact Modern Greece local-history enumerations into the official place
-- spans.  These are leaves, so permissive ranges remain unambiguous.
PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;

CREATE TEMP TABLE _greece_base(id INTEGER NOT NULL);
INSERT INTO _greece_base SELECT COALESCE(MAX(concept_id),0) FROM concept;
CREATE TEMP TABLE _greece_node(seq INTEGER PRIMARY KEY,label TEXT,selector TEXT);
INSERT INTO _greece_node VALUES
 (1,'Regions, Provinces & Islands','DF901.A..DF901.Z'),
 (2,'Athens','DF915..DF936'),
 (3,'Other Cities & Towns','DF951.A..DF951.Z');
INSERT INTO concept(concept_id,preferred_label)
SELECT b.id+n.seq,n.label FROM _greece_node n CROSS JOIN _greece_base b;
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT b.id+n.seq,3568,n.seq FROM _greece_node n CROSS JOIN _greece_base b;
INSERT INTO source_selector(concept_id,system_id,selector)
SELECT b.id+n.seq,'lcc',n.selector FROM _greece_node n CROSS JOIN _greece_base b;

DELETE FROM source_selector
WHERE concept_id=3568 AND system_id='lcc'
  AND (selector GLOB 'DF901.*' OR selector GLOB 'DF951.*'
       OR selector IN ('DF915..DF936','DF935..DF936'));

DROP TABLE _greece_node;
DROP TABLE _greece_base;
COMMIT;

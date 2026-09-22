-- Replace Literary History's overlapping umbrella selectors with official ranged leaves.
PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;

-- Period leaves belong under History, not the generic studies bucket.
DELETE FROM concept_parent WHERE concept_id IN (4379,4380);
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal) VALUES
 (4379,4378,0),(4380,4378,0);

CREATE TEMP TABLE _lh_base(id INTEGER NOT NULL);
INSERT INTO _lh_base SELECT COALESCE(MAX(concept_id),0) FROM concept;
CREATE TEMP TABLE _lh_node(seq INTEGER PRIMARY KEY,label TEXT NOT NULL,selector TEXT NOT NULL);
INSERT INTO _lh_node VALUES
 (1,'Literary Biography','PN451..PN497'),
 (2,'Literary-History Collections','PN500..PN519'),
 (3,'Universal Literary Histories','PN521..PN595'),
 (4,'Literary Movements & Relations','PN596.2..PN605'),
 (5,'Ancient & Early Literary History','PN610..PN660'),
 (6,'Romance Literatures','PN801..PN820'),
 (7,'Germanic Literatures','PN821..PN840'),
 (8,'Regional & National Literatures','PN843..PN849'),
 (9,'Comparative Literature','PN851..PN884'),
 (10,'Literary Translation','PN885.2..PN889'),
 (11,'Folk Literature','PN905..PN1008');

INSERT INTO concept(concept_id,preferred_label)
SELECT b.id+n.seq,n.label FROM _lh_node n CROSS JOIN _lh_base b;
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT b.id+n.seq,4378,0 FROM _lh_node n CROSS JOIN _lh_base b;
INSERT INTO source_selector(concept_id,system_id,selector)
SELECT b.id+n.seq,'lcc',n.selector FROM _lh_node n CROSS JOIN _lh_base b;

DELETE FROM source_selector WHERE concept_id=4378 AND system_id='lcc';
-- Exact schedule points may remain on a nonleaf.
INSERT INTO source_selector VALUES
 (4378,'lcc','PN441'),
 (4378,'lcc','PN841'),
 (4378,'lcc','PN842');

DROP TABLE _lh_node;
DROP TABLE _lh_base;
COMMIT;

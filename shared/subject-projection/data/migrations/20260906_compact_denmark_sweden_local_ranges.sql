-- Compact enumerated Danish and Swedish local-history cutters into official
-- county/province and city leaf ranges.
PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;

CREATE TEMP TABLE _scand_base(id INTEGER NOT NULL);
INSERT INTO _scand_base SELECT COALESCE(MAX(concept_id),0) FROM concept;
CREATE TEMP TABLE _scand_node(seq INTEGER PRIMARY KEY,parent_id INTEGER,label TEXT,selector TEXT);
INSERT INTO _scand_node VALUES
 (1,3580,'Counties, Regions & Islands','DL271.A..DL271.Z'),
 (2,3580,'Cities & Towns','DL276..DL291.Z'),
 (3,3584,'Provinces & Regions','DL971.A..DL971.Z'),
 (4,3584,'Cities & Towns','DL976..DL991.Z');
INSERT INTO concept(concept_id,preferred_label)
SELECT b.id+n.seq,n.label FROM _scand_node n CROSS JOIN _scand_base b;
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT b.id+n.seq,n.parent_id,n.seq FROM _scand_node n CROSS JOIN _scand_base b;
INSERT INTO source_selector(concept_id,system_id,selector)
SELECT b.id+n.seq,'lcc',n.selector FROM _scand_node n CROSS JOIN _scand_base b;

DELETE FROM source_selector WHERE concept_id=3580 AND system_id='lcc'
 AND (selector GLOB 'DL271.*' OR selector GLOB 'DL291.*' OR selector='DL276');
DELETE FROM source_selector WHERE concept_id=3584 AND system_id='lcc'
 AND (selector GLOB 'DL971.*' OR selector GLOB 'DL991.*' OR selector='DL976');

DROP TABLE _scand_node;
DROP TABLE _scand_base;
COMMIT;

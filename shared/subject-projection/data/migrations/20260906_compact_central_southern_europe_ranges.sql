-- Compact verified Austrian, Romanian and Bulgarian local-history cutters,
-- plus Italian biography/historiography enumerations, into ranged leaves.
PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;

CREATE TEMP TABLE _eur_base(id INTEGER NOT NULL);
INSERT INTO _eur_base SELECT COALESCE(MAX(concept_id),0) FROM concept;
CREATE TEMP TABLE _eur_node(seq INTEGER PRIMARY KEY,parent_id INTEGER,label TEXT,selector TEXT);
INSERT INTO _eur_node VALUES
 (1,3537,'Other Cities & Towns','DB879.A..DB879.Z'),
 (2,3548,'Other Cities & Towns','DR296.A..DR296.Z'),
 (3,3545,'Provinces & Regions','DR95.A..DR95.Z'),
 (4,3545,'Cities & Towns','DR97..DR98.Z'),
 (5,3576,'Collective Biography','DG463..DG463.8.Z'),
 (6,3576,'Historiography','DG465..DG465.7.Z');
INSERT INTO concept(concept_id,preferred_label)
SELECT b.id+n.seq,n.label FROM _eur_node n CROSS JOIN _eur_base b;
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT b.id+n.seq,n.parent_id,n.seq FROM _eur_node n CROSS JOIN _eur_base b;
INSERT INTO source_selector(concept_id,system_id,selector)
SELECT b.id+n.seq,'lcc',n.selector FROM _eur_node n CROSS JOIN _eur_base b;

DELETE FROM source_selector WHERE concept_id=3537 AND system_id='lcc' AND selector GLOB 'DB879.*';
DELETE FROM source_selector WHERE concept_id=3548 AND system_id='lcc' AND selector GLOB 'DR296.*';
DELETE FROM source_selector WHERE concept_id=3545 AND system_id='lcc'
 AND (selector GLOB 'DR95.*' OR selector='DR97' OR selector GLOB 'DR98.*');
DELETE FROM source_selector WHERE concept_id=3607 AND system_id='lcc'
 AND (selector='DG463' OR selector GLOB 'DG463.*' OR selector='DG465' OR selector GLOB 'DG465.*');

DROP TABLE _eur_node;
DROP TABLE _eur_base;
COMMIT;

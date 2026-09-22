-- Replace the Australia enumeration with official general, social/geographic,
-- state and territory blocks. Existing broad chronological children remain.
PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;

CREATE TEMP TABLE _aus_base(id INTEGER NOT NULL);
INSERT INTO _aus_base SELECT COALESCE(MAX(concept_id),0) FROM concept;
CREATE TEMP TABLE _aus_node(seq INTEGER PRIMARY KEY,label TEXT,selector TEXT);
INSERT INTO _aus_node VALUES
 (1,'General, Reference & Historiography','DU80..DU113.5'),
 (2,'Society, Culture & Australian Capital Territory','DU118..DU145'),
 (3,'New South Wales','DU150..DU180'),
 (4,'Tasmania','DU182..DU198'),
 (5,'Victoria','DU200..DU230'),
 (6,'Queensland','DU250..DU280'),
 (7,'South Australia','DU300..DU330'),
 (8,'Western Australia','DU350..DU380'),
 (9,'Central Australia & Northern Territory','DU390..DU398');
INSERT INTO concept(concept_id,preferred_label)
SELECT b.id+n.seq,n.label FROM _aus_node n CROSS JOIN _aus_base b;
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT b.id+n.seq,3677,
       COALESCE((SELECT MAX(ordinal)+1 FROM concept_parent WHERE parent_concept_id=3677),0)+n.seq-1
FROM _aus_node n CROSS JOIN _aus_base b;
INSERT INTO source_selector(concept_id,system_id,selector)
SELECT b.id+n.seq,'lcc',n.selector FROM _aus_node n CROSS JOIN _aus_base b;

DELETE FROM source_selector WHERE concept_id=3677 AND system_id='lcc';

DROP TABLE _aus_node;
DROP TABLE _aus_base;
COMMIT;

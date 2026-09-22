-- Restrict Eastern Europe to its own DJK general schedule. DK national and
-- former-Soviet schedules belong to country/region descendants, not this umbrella.
PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;

-- Flatten the selectorless chronological navigation node.
UPDATE concept_parent SET parent_concept_id=3522 WHERE parent_concept_id=11441;
DELETE FROM concept WHERE concept_id=11441;

DELETE FROM source_selector WHERE concept_id=3522 AND system_id='lcc';
INSERT INTO source_selector VALUES(3522,'lcc','DJK');

CREATE TEMP TABLE _east_base(id INTEGER NOT NULL);
INSERT INTO _east_base SELECT COALESCE(MAX(concept_id),0) FROM concept;
CREATE TEMP TABLE _east_node(
 seq INTEGER PRIMARY KEY,parent_id INTEGER,label TEXT,selector_a TEXT,selector_b TEXT,selector_c TEXT
);
INSERT INTO _east_node VALUES
 (1,3522,'General & Reference','DJK1..DJK12',NULL,NULL),
 (2,3522,'Culture & Ethnography','DJK23..DJK28.Z',NULL,NULL),
 (3,3522,'General Historical Studies','DJK29.2..DJK45.Z',NULL,NULL),
 (4,3522,'Black Sea Region','DJK61..DJK62.Z','DJK64..DJK66',NULL),
 (5,3522,'Carpathian Mountain Region','DJK71..DJK72.Z','DJK74..DJK76.1',NULL),
 (6,3522,'Danube River Valley','DJK76.2..DJK76.3.Z','DJK76.5..DJK76.8',NULL),
 (7,3522,'Pannonia','DJK77',NULL,NULL),
 (8,2455,'Eastern Europe','DJK13..DJK19','DJK63','DJK73'),
 (9,2455,'Danube River Valley','DJK76.4',NULL,NULL);
INSERT INTO concept(concept_id,preferred_label)
SELECT b.id+n.seq,n.label FROM _east_node n CROSS JOIN _east_base b;
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT b.id+n.seq,n.parent_id,
       COALESCE((SELECT MAX(ordinal)+1 FROM concept_parent WHERE parent_concept_id=n.parent_id),0)+n.seq
FROM _east_node n CROSS JOIN _east_base b;
INSERT INTO source_selector(concept_id,system_id,selector)
SELECT b.id+n.seq,'lcc',n.selector_a FROM _east_node n CROSS JOIN _east_base b;
INSERT INTO source_selector(concept_id,system_id,selector)
SELECT b.id+n.seq,'lcc',n.selector_b FROM _east_node n CROSS JOIN _east_base b WHERE n.selector_b IS NOT NULL;
INSERT INTO source_selector(concept_id,system_id,selector)
SELECT b.id+n.seq,'lcc',n.selector_c FROM _east_node n CROSS JOIN _east_base b WHERE n.selector_c IS NOT NULL;

DROP TABLE _east_node;
DROP TABLE _east_base;
COMMIT;

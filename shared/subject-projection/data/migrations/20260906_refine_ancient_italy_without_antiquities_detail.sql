-- Keep antiquities broad; add detail only to local history and major imperial eras.
PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;

CREATE TEMP TABLE _ai2_target(kind TEXT PRIMARY KEY,id INTEGER NOT NULL);
INSERT INTO _ai2_target
SELECT 'antiquities',c.concept_id
FROM concept_parent cp JOIN concept c ON c.concept_id=cp.concept_id
WHERE cp.parent_concept_id=3571 AND c.preferred_label='Antiquities, Civilization & Culture';
INSERT INTO _ai2_target
SELECT 'local',c.concept_id
FROM concept_parent cp JOIN concept c ON c.concept_id=cp.concept_id
WHERE cp.parent_concept_id=3571 AND c.preferred_label='Local History';

-- Collapse the previously expanded antiquities subtree into one deliberate leaf.
DELETE FROM concept
WHERE concept_id IN (
 SELECT cp.concept_id FROM concept_parent cp
 JOIN _ai2_target t ON t.kind='antiquities' AND t.id=cp.parent_concept_id
);
INSERT OR IGNORE INTO source_selector(concept_id,system_id,selector)
SELECT id,'lcc','DG74.2..DG142.24@3' FROM _ai2_target WHERE kind='antiquities';

CREATE TEMP TABLE _ai2_base(id INTEGER NOT NULL);
INSERT INTO _ai2_base SELECT COALESCE(MAX(concept_id),0) FROM concept;
CREATE TEMP TABLE _ai2_node(seq INTEGER PRIMARY KEY,parent_id INTEGER NOT NULL,label TEXT NOT NULL,ordinal INTEGER NOT NULL,selector TEXT NOT NULL);

INSERT INTO _ai2_node
SELECT 1,id,'Regions & Provinces',0,'DG51..DG59@4' FROM _ai2_target
WHERE kind='local' AND EXISTS(SELECT 1 FROM source_selector WHERE concept_id=id AND selector='DG51..DG70@3');
INSERT INTO _ai2_node
SELECT 2,id,'Rome to 476',1,'DG61..DG69@4' FROM _ai2_target
WHERE kind='local' AND EXISTS(SELECT 1 FROM source_selector WHERE concept_id=id AND selector='DG51..DG70@3');
INSERT INTO _ai2_node
SELECT 3,id,'Other Cities & Towns',2,'DG70.A..DG70.Z@4' FROM _ai2_target
WHERE kind='local' AND EXISTS(SELECT 1 FROM source_selector WHERE concept_id=id AND selector='DG51..DG70@3');

INSERT INTO _ai2_node
SELECT 4,10217,'Twelve Caesars, 27 BCE–96 CE',0,'DG277..DG291@5'
WHERE EXISTS(SELECT 1 FROM source_selector WHERE concept_id=10217 AND selector='DG275..DG309.3@4');
INSERT INTO _ai2_node
SELECT 5,10217,'High Empire, 96–180',1,'DG292..DG297@5'
WHERE EXISTS(SELECT 1 FROM source_selector WHERE concept_id=10217 AND selector='DG275..DG309.3@4');
INSERT INTO _ai2_node
SELECT 6,10217,'Later Principate, 180–284',2,'DG298..DG309.3@5'
WHERE EXISTS(SELECT 1 FROM source_selector WHERE concept_id=10217 AND selector='DG275..DG309.3@4');

INSERT INTO concept(concept_id,preferred_label)
SELECT b.id+n.seq,n.label FROM _ai2_node n CROSS JOIN _ai2_base b;
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal,display_order)
SELECT b.id+n.seq,n.parent_id,n.ordinal,n.ordinal FROM _ai2_node n CROSS JOIN _ai2_base b;
INSERT INTO source_selector(concept_id,system_id,selector)
SELECT b.id+n.seq,'lcc',n.selector FROM _ai2_node n CROSS JOIN _ai2_base b;

DELETE FROM source_selector
WHERE selector='DG51..DG70@3'
  AND concept_id IN (SELECT id FROM _ai2_target WHERE kind='local');
DELETE FROM source_selector WHERE concept_id=10217 AND selector='DG275..DG309.3@4';
INSERT OR IGNORE INTO source_selector VALUES(10217,'lcc','DG275');
INSERT OR IGNORE INTO source_selector VALUES(10217,'lcc','DG276');
INSERT OR IGNORE INTO source_selector VALUES(10217,'lcc','DG276.5');

DROP TABLE _ai2_node;
DROP TABLE _ai2_base;
DROP TABLE _ai2_target;
COMMIT;

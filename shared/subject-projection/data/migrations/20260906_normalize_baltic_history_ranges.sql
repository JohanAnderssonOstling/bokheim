-- Compact the Baltic-general, Estonian, Latvian, and Lithuanian schedules.
PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;

-- Existing capital-city leaves were mislabeled as their countries.
UPDATE concept SET preferred_label='Tallinn' WHERE concept_id=3588;
UPDATE concept SET preferred_label='Riga' WHERE concept_id=3592;
UPDATE concept SET preferred_label='Vilnius' WHERE concept_id=3593;

DELETE FROM source_selector WHERE system_id='lcc' AND concept_id IN
 (3519,11216,11217,11221,3588,3592,3593);
INSERT INTO source_selector VALUES
 (3588,'lcc','DK503.92..DK503.939'),
 (3592,'lcc','DK504.92..DK504.939'),
 (3593,'lcc','DK505.92..DK505.939');

CREATE TEMP TABLE _baltic_base(id INTEGER NOT NULL);
INSERT INTO _baltic_base SELECT COALESCE(MAX(concept_id),0) FROM concept;
CREATE TEMP TABLE _baltic_node(
 seq INTEGER PRIMARY KEY,parent_id INTEGER,label TEXT,selector_a TEXT,selector_b TEXT,selector_c TEXT
);
INSERT INTO _baltic_node VALUES
 (1,3519,'General Baltic History & Culture','DK502.3..DK502.75',NULL,NULL),
 (2,11216,'General, Culture & Historical Studies','DK503..DK503.21.Z','DK503.23..DK503.28.Z','DK503.3..DK503.69.Z'),
 (3,11216,'Early Estonia, to 1800','DK503.7..DK503.72',NULL,NULL),
 (4,11216,'Regions','DK503.9.A..DK503.9.Z',NULL,NULL),
 (5,11216,'Other Cities & Towns','DK503.95.A..DK503.95.Z',NULL,NULL),
 (6,11217,'General, Culture & Historical Studies','DK504..DK504.21.Z','DK504.23..DK504.28.Z','DK504.3..DK504.69.Z'),
 (7,11217,'Early Latvia, to 1800','DK504.7..DK504.72',NULL,NULL),
 (8,11217,'Regions','DK504.9.A..DK504.9.Z',NULL,NULL),
 (9,11217,'Other Cities & Towns','DK504.95.A..DK504.95.Z',NULL,NULL),
 (10,11221,'General, Culture & Historical Studies','DK505..DK505.21.Z','DK505.23..DK505.28.Z','DK505.3..DK505.69.Z'),
 (11,11221,'Regions','DK505.9.A..DK505.9.Z',NULL,NULL),
 (12,11221,'Other Cities & Towns','DK505.95.A..DK505.95.Z',NULL,NULL),
 (13,2455,'Estonia','DK503.22','DK503.29',NULL),
 (14,2455,'Latvia','DK504.22','DK504.29',NULL),
 (15,2455,'Lithuania','DK505.22','DK505.29',NULL);
INSERT INTO concept(concept_id,preferred_label)
SELECT b.id+n.seq,n.label FROM _baltic_node n CROSS JOIN _baltic_base b;
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT b.id+n.seq,n.parent_id,
       COALESCE((SELECT MAX(ordinal)+1 FROM concept_parent WHERE parent_concept_id=n.parent_id),0)+n.seq
FROM _baltic_node n CROSS JOIN _baltic_base b;
INSERT INTO source_selector(concept_id,system_id,selector)
SELECT b.id+n.seq,'lcc',n.selector_a FROM _baltic_node n CROSS JOIN _baltic_base b;
INSERT INTO source_selector(concept_id,system_id,selector)
SELECT b.id+n.seq,'lcc',n.selector_b FROM _baltic_node n CROSS JOIN _baltic_base b WHERE n.selector_b IS NOT NULL;
INSERT INTO source_selector(concept_id,system_id,selector)
SELECT b.id+n.seq,'lcc',n.selector_c FROM _baltic_node n CROSS JOIN _baltic_base b WHERE n.selector_c IS NOT NULL;

DROP TABLE _baltic_node;
DROP TABLE _baltic_base;
COMMIT;

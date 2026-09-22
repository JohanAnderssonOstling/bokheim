-- Normalize Belgium to broad official DH schedule blocks.
PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;

CREATE TEMP TABLE _belgium_base(id INTEGER NOT NULL);
INSERT INTO _belgium_base SELECT COALESCE(MAX(concept_id),0) FROM concept;
INSERT INTO concept(concept_id,preferred_label) SELECT id+1,'Belgium' FROM _belgium_base;
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT id+1,2455,COALESCE((SELECT MAX(ordinal)+1 FROM concept_parent WHERE parent_concept_id=2455),0)
FROM _belgium_base;
INSERT INTO source_selector SELECT id+1,'lcc','DH431..DH435' FROM _belgium_base;

DELETE FROM source_selector
WHERE system_id='lcc' AND concept_id IN (3577,10148,10135,10207,13751,13752,13753);
INSERT INTO source_selector VALUES
 (10148,'lcc','DH611..DH619'),
 (10135,'lcc','DH620..DH644.Z'),
 (10207,'lcc','DH645..DH676'),
 (13751,'lcc','DH801.A..DH801.Z'),
 (13752,'lcc','DH802..DH809.811'),
 (13753,'lcc','DH811.A..DH811.Z');
UPDATE concept SET preferred_label='French Rule & United Netherlands, 1794–1830' WHERE concept_id=10135;
UPDATE concept SET preferred_label='Independent Belgium, 1830–1909' WHERE concept_id=10207;

CREATE TEMP TABLE _belgium_node(seq INTEGER PRIMARY KEY,label TEXT,selector_a TEXT,selector_b TEXT);
INSERT INTO _belgium_node VALUES
 (2,'General & Reference','DH401..DH430.5',NULL),
 (3,'Antiquities, Culture & Ethnography','DH451..DH492.Z',NULL),
 (4,'General Historical Studies','DH503..DH569.Z',NULL),
 (5,'Early & Medieval Belgium, to 1555','DH571..DH584',NULL),
 (6,'Spanish Netherlands, 1555–1714','DH585..DH610.Z',NULL),
 (7,'Belgium in the 20th Century','DH677..DH694',NULL),
 (8,'Belgium in the 21st Century','DH695..DH697',NULL);
INSERT INTO concept(concept_id,preferred_label)
SELECT b.id+n.seq,n.label FROM _belgium_node n CROSS JOIN _belgium_base b;
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT b.id+n.seq,3577,
       COALESCE((SELECT MAX(ordinal)+1 FROM concept_parent WHERE parent_concept_id=3577),0)+n.seq-2
FROM _belgium_node n CROSS JOIN _belgium_base b;
INSERT INTO source_selector(concept_id,system_id,selector)
SELECT b.id+n.seq,'lcc',n.selector_a FROM _belgium_node n CROSS JOIN _belgium_base b;

DROP TABLE _belgium_node;
DROP TABLE _belgium_base;
COMMIT;

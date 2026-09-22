-- Replace hundreds of enumerated Ukraine schedule points with broad official
-- ranges, while preserving mutually exclusive eras and major localities.
PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;

DELETE FROM source_selector WHERE system_id='lcc' AND concept_id IN
 (3603,11381,11361,11393,11388,11395,11396);
INSERT INTO source_selector VALUES
 (11393,'lcc','DK508.66'),
 (11393,'lcc','DK5205..DK5219.Z'),
 (11396,'lcc','DK508.67..DK508.675'),
 (11396,'lcc','DK5223..DK5237.Z'),
 (11395,'lcc','DK508.68..DK508.695'),
 (11395,'lcc','DK5241..DK5255.Z'),
 (11361,'lcc','DK508.73..DK508.761'),
 (11361,'lcc','DK5259..DK5273.Z'),
 (11388,'lcc','DK508.77..DK508.782'),
 (11388,'lcc','DK5277..DK5303.Z'),
 (11381,'lcc','DK508.788..DK508.852'),
 (11381,'lcc','DK5309..DK5381.Z');

CREATE TEMP TABLE _ukraine_base(id INTEGER NOT NULL);
INSERT INTO _ukraine_base SELECT COALESCE(MAX(concept_id),0) FROM concept;
CREATE TEMP TABLE _ukraine_node(
 seq INTEGER PRIMARY KEY,parent_id INTEGER,label TEXT,selector_a TEXT,selector_b TEXT
);
INSERT INTO _ukraine_node VALUES
 (1,3603,'General, Culture & Historical Studies','DK508..DK508.65.Z',NULL),
 (2,3603,'Ukraine, 1991–2014','DK5385..DK5403.Z',NULL),
 (3,3603,'Ukraine, 2014–Present','DK5409..DK5529.Z',NULL),
 (4,3603,'Regions & Oblasts','DK508.9.A..DK508.9.Z','DK5850.A..DK5850.Z'),
 (5,3603,'Kyiv','DK508.92..DK508.939.Z','DK5860..DK5878.5.Z'),
 (6,3603,'Kharkiv','DK5880..DK5898.5.Z',NULL),
 (7,3603,'Lviv','DK5900..DK5918.5.Z',NULL),
 (8,3603,'Odesa','DK5920..DK5938.5.Z',NULL),
 (9,3603,'Other Cities & Towns','DK508.95.A1..DK508.95.Z','DK5990..DK5995.Z'),
 (10,3603,'General & Reference','DK5001..DK5019',NULL),
 (11,3603,'Antiquities, Culture & Ethnography','DK5071..DK5099',NULL),
 (12,3603,'General Historical Studies','DK5111..DK5185.Z',NULL),
 (13,2455,'Ukraine','DK508.A5..DK508.Z','DK5021..DK5051');
INSERT INTO concept(concept_id,preferred_label)
SELECT b.id+n.seq,n.label FROM _ukraine_node n CROSS JOIN _ukraine_base b;
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT b.id+n.seq,n.parent_id,
       COALESCE((SELECT MAX(ordinal)+1 FROM concept_parent WHERE parent_concept_id=n.parent_id),0)+n.seq
FROM _ukraine_node n CROSS JOIN _ukraine_base b;
INSERT INTO source_selector(concept_id,system_id,selector)
SELECT b.id+n.seq,'lcc',n.selector_a FROM _ukraine_node n CROSS JOIN _ukraine_base b;
INSERT INTO source_selector(concept_id,system_id,selector)
SELECT b.id+n.seq,'lcc',n.selector_b FROM _ukraine_node n CROSS JOIN _ukraine_base b WHERE n.selector_b IS NOT NULL;

DROP TABLE _ukraine_node;
DROP TABLE _ukraine_base;
COMMIT;

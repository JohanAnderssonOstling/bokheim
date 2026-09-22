-- Normalize Germany to the broad official DD schedule blocks.  Chronological
-- leaves are mutually exclusive, and description/travel leaves the history tree.
PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;

CREATE TEMP TABLE _germany_base(id INTEGER NOT NULL);
INSERT INTO _germany_base SELECT COALESCE(MAX(concept_id),0) FROM concept;

-- Add guidebooks to the existing Travel > Europe > Germany leaf.
INSERT OR IGNORE INTO source_selector VALUES(2460,'lcc','DD16');
INSERT OR IGNORE INTO source_selector VALUES(2460,'lcc','DD21.5..DD43');

-- Remove overlapping/redundant selector sets and retain one official span per era.
DELETE FROM source_selector
WHERE system_id='lcc' AND concept_id IN
 (3524,10121,3557,3560,10222,3561,3563,3559,3564,3558,13706,3562,
  13703,13704,13705,11448);
INSERT INTO source_selector VALUES
 (10121,'lcc','DD121..DD124'),
 (3557,'lcc','DD125..DD174.6'),
 (3560,'lcc','DD175..DD199'),
 (10222,'lcc','DD201..DD216'),
 (3561,'lcc','DD217..DD231'),
 (3563,'lcc','DD232..DD252.Z'),
 (3559,'lcc','DD253..DD256.824'),
 (3564,'lcc','DD258..DD279'),
 (3558,'lcc','DD280..DD289.5'),
 (13706,'lcc','DD290..DD290.33.Z'),
 (3562,'lcc','DD301..DD454.2'),
 (13703,'lcc','DD51..DD55.2'),
 (13704,'lcc','DD60..DD68'),
 (13705,'lcc','DD73..DD78.Z'),
 (11448,'lcc','DD851..DD900');

CREATE TEMP TABLE _germany_node(seq INTEGER PRIMARY KEY,label TEXT,selector_a TEXT,selector_b TEXT);
INSERT INTO _germany_node VALUES
 (2,'General & Reference','DD1..DD15.5','DD17..DD21.3'),
 (3,'General Historical Studies','DD84..DD120',NULL),
 (4,'Allied Occupation, 1945–1949','DD257..DD257.42',NULL),
 (5,'Broad Regions','DD701..DD789',NULL),
 (6,'Bonn','DD900.2..DD900.76',NULL),
 (7,'Other Cities & Towns','DD901.A..DD905.22',NULL);
INSERT INTO concept(concept_id,preferred_label)
SELECT b.id+n.seq,n.label FROM _germany_node n CROSS JOIN _germany_base b;
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT b.id+n.seq,3524,
       COALESCE((SELECT MAX(ordinal)+1 FROM concept_parent WHERE parent_concept_id=3524),0)+n.seq-2
FROM _germany_node n CROSS JOIN _germany_base b;
INSERT INTO source_selector(concept_id,system_id,selector)
SELECT b.id+n.seq,'lcc',n.selector_a FROM _germany_node n CROSS JOIN _germany_base b;
INSERT INTO source_selector(concept_id,system_id,selector)
SELECT b.id+n.seq,'lcc',n.selector_b FROM _germany_node n CROSS JOIN _germany_base b
WHERE n.selector_b IS NOT NULL;

DROP TABLE _germany_node;
DROP TABLE _germany_base;
COMMIT;

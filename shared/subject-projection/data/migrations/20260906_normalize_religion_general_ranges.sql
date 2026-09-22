-- Normalize the BL General & Reference subtree into mutually exclusive leaf
-- ranges and remove the covered parent enumeration.
PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;

CREATE TEMP TABLE _relig_ranges(id INTEGER,selector TEXT);
INSERT INTO _relig_ranges VALUES
 (10945,'BL53..BL54.99'),(11632,'BL55..BL69.99'),
 (11633,'BL70..BL73.99'),(11634,'BL74..BL99.99'),
 (11636,'BL300..BL349.99'),(11637,'BL350..BL424.99'),
 (11638,'BL425..BL499.99'),(11639,'BL500..BL549.99'),
 (11640,'BL550..BL623.99'),(11641,'BL624..BL629.99'),
 (11642,'BL630..BL659.99'),(11645,'BL1600..BL1699.99'),
 (11646,'BL2390..BL2499.99');
DELETE FROM source_selector WHERE system_id='lcc'
 AND concept_id IN (SELECT DISTINCT id FROM _relig_ranges);
INSERT INTO source_selector(concept_id,system_id,selector)
SELECT id,'lcc',selector FROM _relig_ranges;

-- General/other Asian material is the complement of the specific country
-- ranges already assigned to China, India, Japan and Korea.
DELETE FROM source_selector WHERE concept_id=12843 AND system_id='lcc';
INSERT INTO source_selector VALUES
 (12843,'lcc','BL1000..BL1099.99'),
 (12843,'lcc','BL1700..BL1789.99'),
 (12843,'lcc','BL1975.1..BL1999.99'),
 (12843,'lcc','BL2032.1..BL2194.99'),
 (12843,'lcc','BL2240.1..BL2389.99');

CREATE TEMP TABLE _relig_base(id INTEGER NOT NULL);
INSERT INTO _relig_base SELECT COALESCE(MAX(concept_id),0) FROM concept;
CREATE TEMP TABLE _relig_node(seq INTEGER PRIMARY KEY,parent_id INTEGER,label TEXT,selector TEXT);
INSERT INTO _relig_node VALUES
 (1,5529,'Religion—General & Reference','BL1..BL52.9'),
 (3,5529,'Indigenous Religions of the Americas','BL2500..BL2592'),
 (4,5529,'Oceanian Religions','BL2600..BL2630'),
 (5,5529,'Arctic Religions','BL2670..BL2670.22');
INSERT INTO concept(concept_id,preferred_label)
SELECT b.id+n.seq,n.label FROM _relig_node n CROSS JOIN _relig_base b;
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT b.id+n.seq,n.parent_id,n.seq FROM _relig_node n CROSS JOIN _relig_base b;
INSERT INTO source_selector(concept_id,system_id,selector)
SELECT b.id+n.seq,'lcc',n.selector FROM _relig_node n CROSS JOIN _relig_base b;

-- Extend the existing residual European leaf through the end of the official
-- European schedule rather than creating a duplicate subject.
DELETE FROM source_selector WHERE concept_id=12854 AND system_id='lcc';
INSERT INTO source_selector VALUES(12854,'lcc','BL900..BL980');

-- Remove direct selectors covered by the normalized descendant ranges.
DELETE FROM source_selector WHERE concept_id=5529 AND system_id='lcc'
 AND selector GLOB 'BL[0-9]*' AND (
   CAST(substr(selector,3) AS REAL) BETWEEN 1 AND 99.999
   OR CAST(substr(selector,3) AS REAL) BETWEEN 100 AND 659.999
   OR CAST(substr(selector,3) AS REAL) BETWEEN 689 AND 1099.999
   OR CAST(substr(selector,3) AS REAL) BETWEEN 1600 AND 2670.229
   OR CAST(substr(selector,3) AS REAL) BETWEEN 2700 AND 2790.999
 );

DROP TABLE _relig_node;
DROP TABLE _relig_base;
DROP TABLE _relig_ranges;
COMMIT;

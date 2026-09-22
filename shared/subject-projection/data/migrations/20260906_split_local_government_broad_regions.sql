-- Replace the JS umbrella with broad topical/geographic leaves.  The relatively
-- busy United States branch receives one additional official level.
PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

CREATE TEMP TABLE _lg_active(ok INTEGER PRIMARY KEY);
INSERT INTO _lg_active
SELECT 1 WHERE EXISTS (
  SELECT 1 FROM source_selector
  WHERE concept_id=5359 AND system_id='lcc' AND selector='JS39..JS8500@1'
);
CREATE TEMP TABLE _lg_base(id INTEGER NOT NULL);
INSERT INTO _lg_base SELECT COALESCE(MAX(concept_id),0) FROM concept;
CREATE TEMP TABLE _lg_node(
 seq INTEGER PRIMARY KEY,parent_seq INTEGER,label TEXT NOT NULL,ordinal INTEGER NOT NULL
);
INSERT INTO _lg_node VALUES
 (1,NULL,'General & Reference',0),
 (2,NULL,'Municipal Institutions & Politics',1),
 (3,NULL,'United States',2),
 (4,3,'Municipal Government',0),
 (5,3,'Other Local Government',1),
 (6,3,'States & Localities',2),
 (7,NULL,'Canada',3),
 (8,NULL,'Caribbean',4),
 (9,NULL,'Latin America',5),
 (10,NULL,'Europe',6),
 (11,NULL,'Asia',7),
 (12,NULL,'Africa',8),
 (13,NULL,'Oceania & Pacific',9),
 (14,NULL,'Polar Regions',10);

INSERT INTO concept(concept_id,preferred_label)
SELECT b.id+n.seq,n.label FROM _lg_node n CROSS JOIN _lg_base b JOIN _lg_active;
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal,display_order)
SELECT b.id+n.seq,
       CASE WHEN n.parent_seq IS NULL THEN 5359 ELSE b.id+n.parent_seq END,
       n.ordinal,n.ordinal
FROM _lg_node n CROSS JOIN _lg_base b JOIN _lg_active;

CREATE TEMP TABLE _lg_selector(seq INTEGER NOT NULL,selector TEXT NOT NULL);
INSERT INTO _lg_selector VALUES
 (1,'JS39..JS113@2'),
 (2,'JS141..JS271@2'),
 (4,'JS300..JS401@3'),
 (5,'JS408..JS426@3'),
 (6,'JS431..JS1583@3'),
 (7,'JS1701..JS1819@2'),
 (8,'JS1830..JS2058.9@2'),
 (9,'JS2061..JS2778@2'),
 (10,'JS3000..JS6949.89@2'),
 (11,'JS6950..JS7520@2'),
 (12,'JS7525..JS7819.9@2'),
 (13,'JS7820..JS7906.9@2'),
 (13,'JS8001..JS8490.9@2'),
 (14,'JS8495..JS8499.9@2');
INSERT INTO source_selector(concept_id,system_id,selector)
SELECT b.id+s.seq,'lcc',s.selector
FROM _lg_selector s CROSS JOIN _lg_base b JOIN _lg_active;

-- JS is the literal class selector; JS8500 is the exact developing-countries
-- number. Both may remain directly on the now non-leaf parent.
INSERT OR IGNORE INTO source_selector(concept_id,system_id,selector)
SELECT 5359,'lcc','JS8500' FROM _lg_active;
DELETE FROM source_selector
WHERE concept_id=5359 AND system_id='lcc' AND selector='JS39..JS8500@1';

DROP TABLE _lg_selector;
DROP TABLE _lg_node;
DROP TABLE _lg_base;
DROP TABLE _lg_active;
COMMIT;

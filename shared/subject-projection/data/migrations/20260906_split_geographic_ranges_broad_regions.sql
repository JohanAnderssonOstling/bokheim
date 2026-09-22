-- Split broad geographic leaves at official LCC regional boundaries.  Country
-- detail is intentionally limited to the high-use North American branches.
PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

CREATE TEMP TABLE _geo_base(id INTEGER NOT NULL);
INSERT INTO _geo_base SELECT COALESCE(MAX(concept_id), 0) FROM concept;

CREATE TEMP TABLE _geo_node(
  seq INTEGER PRIMARY KEY,
  root_id INTEGER NOT NULL,
  parent_seq INTEGER,
  parent_id INTEGER,
  label TEXT NOT NULL,
  ordinal INTEGER NOT NULL
);

-- Armies by Country (UA21-UA885 is the official geographic schedule).
INSERT INTO _geo_node VALUES
 (1,12420,NULL,12420,'North America',0),
 (2,12420,1,NULL,'United States',0),
 (3,12420,1,NULL,'Canada',1),
 (4,12420,1,NULL,'Mexico',2),
 (5,12420,1,NULL,'Central America',3),
 (6,12420,1,NULL,'West Indies',4),
 (7,12420,NULL,12420,'South America',1),
 (8,12420,NULL,12420,'Europe',2),
 (9,12420,NULL,12420,'Asia',3),
 (10,12420,NULL,12420,'Africa',4),
 (11,12420,NULL,12420,'Oceania & Pacific',5),
 (12,12420,NULL,12420,'Arctic Regions',6);

-- National biography by region or country.
INSERT INTO _geo_node VALUES
 (13,6760,NULL,6760,'America',0),
 (14,6760,13,NULL,'North America',0),
 (15,6760,13,NULL,'Caribbean',1),
 (16,6760,13,NULL,'Latin America',2),
 (17,6760,NULL,6760,'Europe',1),
 (18,6760,NULL,6760,'Asia',2),
 (19,6760,NULL,6760,'Africa',3),
 (20,6760,NULL,6760,'Oceania & Pacific',4);

-- Folklore by region or country.  Split North America because its U.S. range
-- is heavily used; retain the other geographic areas as broad leaves.
INSERT INTO _geo_node VALUES
 (21,5062,NULL,5062,'North America',0),
 (22,5062,21,NULL,'United States',0),
 (23,5062,21,NULL,'Canada',1),
 (24,5062,NULL,5062,'Latin America',1),
 (25,5062,NULL,5062,'Europe',2),
 (26,5062,NULL,5062,'Mediterranean Region',3),
 (27,5062,NULL,5062,'Asia',4),
 (28,5062,NULL,5062,'Africa',5),
 (29,5062,NULL,5062,'Oceania & Pacific',6),
 (30,5062,NULL,5062,'Atlantic & Indian Ocean Islands',7),
 (31,5062,NULL,5062,'Arctic Regions',8);

-- Applying a migration twice must not duplicate its subtree.  The roots are
-- active only while they still own their original umbrella selector.
CREATE TEMP TABLE _geo_active(root_id INTEGER PRIMARY KEY);
INSERT INTO _geo_active
SELECT 12420 WHERE EXISTS (
  SELECT 1 FROM source_selector WHERE concept_id=12420 AND system_id='lcc' AND selector='UA21..UA909.9@1'
);
INSERT INTO _geo_active
SELECT 6760 WHERE EXISTS (
  SELECT 1 FROM source_selector WHERE concept_id=6760 AND system_id='lcc' AND selector='CT208..CT3090@4'
);
INSERT INTO _geo_active
SELECT 5062 WHERE EXISTS (
  SELECT 1 FROM source_selector WHERE concept_id=5062 AND system_id='lcc' AND selector='GR99.6..GR390@2'
);

INSERT INTO concept(concept_id, preferred_label)
SELECT b.id+n.seq, n.label
FROM _geo_node n CROSS JOIN _geo_base b
JOIN _geo_active a ON a.root_id=n.root_id;

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal,display_order)
SELECT b.id+n.seq,
       CASE WHEN n.parent_seq IS NULL THEN n.parent_id ELSE b.id+n.parent_seq END,
       n.ordinal,n.ordinal
FROM _geo_node n CROSS JOIN _geo_base b
JOIN _geo_active a ON a.root_id=n.root_id;

CREATE TEMP TABLE _geo_selector(seq INTEGER NOT NULL, selector TEXT NOT NULL);
INSERT INTO _geo_selector VALUES
 -- Armies
 (1,'UA22'),(2,'UA23.A1..UA585@3'),(3,'UA600..UA602@3'),
 (4,'UA603..UA605@3'),(5,'UA606..UA608@3'),(6,'UA609..UA611@3'),
 (7,'UA612..UA645@2'),(8,'UA646..UA829@2'),(9,'UA830..UA854.8@2'),
 (10,'UA855..UA869.Z@2'),(11,'UA870..UA876.2@2'),(12,'UA880..UA880.1@2'),
 -- Biography
 (13,'CT208'),(14,'CT210..CT311.2@3'),(15,'CT329..CT448@3'),
 (16,'CT500..CT506@3'),(17,'CT759..CT1495@2'),(18,'CT1498..CT1919@2'),
 (19,'CT1920..CT2750@2'),(20,'CT2775..CT3090@2'),
 -- Folklore
 (21,'GR101'),(22,'GR105..GR112@3'),(23,'GR113..GR113.7@3'),
 (24,'GR114..GR133@2'),(25,'GR135..GR263@2'),(26,'GR264'),
 (27,'GR265..GR345@2'),(28,'GR350..GR359.52@2'),
 (29,'GR365..GR385@2'),(30,'GR360.A..GR360.Z@2'),(31,'GR390');

INSERT INTO source_selector(concept_id,system_id,selector)
SELECT b.id+s.seq,'lcc',s.selector
FROM _geo_selector s CROSS JOIN _geo_base b
JOIN _geo_node n ON n.seq=s.seq
JOIN _geo_active a ON a.root_id=n.root_id;

-- General single codes remain valid on non-leaf parents.
INSERT OR IGNORE INTO source_selector(concept_id,system_id,selector)
SELECT 12420,'lcc','UA21' WHERE EXISTS (SELECT 1 FROM _geo_active WHERE root_id=12420);
INSERT OR IGNORE INTO source_selector(concept_id,system_id,selector)
SELECT 12420,'lcc','UA885' WHERE EXISTS (SELECT 1 FROM _geo_active WHERE root_id=12420);
INSERT OR IGNORE INTO source_selector(concept_id,system_id,selector)
SELECT 5062,'lcc','GR99.6' WHERE EXISTS (SELECT 1 FROM _geo_active WHERE root_id=5062);
INSERT OR IGNORE INTO source_selector(concept_id,system_id,selector)
SELECT 5062,'lcc','GR100' WHERE EXISTS (SELECT 1 FROM _geo_active WHERE root_id=5062);

DELETE FROM source_selector
WHERE (concept_id=12420 AND system_id='lcc' AND selector='UA21..UA909.9@1')
   OR (concept_id=6760 AND system_id='lcc' AND selector='CT208..CT3090@4')
   OR (concept_id=5062 AND system_id='lcc' AND selector='GR99.6..GR390@2');

DROP TABLE _geo_selector;
DROP TABLE _geo_active;
DROP TABLE _geo_node;
DROP TABLE _geo_base;
COMMIT;

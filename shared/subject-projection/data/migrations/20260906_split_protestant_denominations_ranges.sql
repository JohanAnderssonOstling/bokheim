-- Split major Protestant denomination schedules into broad ranged leaves and
-- remove their enumerated copies from the Protestantism parent.
PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;

DELETE FROM source_selector WHERE concept_id=11478 AND system_id='lcc';
INSERT INTO source_selector VALUES(11478,'lcc','BX5001..BX6093');
DELETE FROM source_selector WHERE concept_id=11491 AND system_id='lcc';
INSERT INTO source_selector VALUES(11491,'lcc','BX8901..BX9225');

CREATE TEMP TABLE _prot_base(id INTEGER NOT NULL);
INSERT INTO _prot_base SELECT COALESCE(MAX(concept_id),0) FROM concept;
CREATE TEMP TABLE _prot_node(seq INTEGER PRIMARY KEY,label TEXT,selector TEXT);
INSERT INTO _prot_node VALUES
 (1,'Adventists','BX6101..BX6193'),
 (2,'Arminians','BX6195..BX6197'),
 (3,'Baptists','BX6201..BX6495'),
 (4,'Catholic Apostolic Church','BX6551..BX6593'),
 (5,'Christadelphians','BX6651..BX6693'),
 (6,'Christian Church','BX6751..BX6793'),
 (7,'Christian Reformed Church','BX6801..BX6843'),
 (8,'Christian Science','BX6901..BX6997'),
 (9,'Churches of God & Churches of Christ','BX7000..BX7099.Z'),
 (10,'Congregationalism','BX7101..BX7260'),
 (11,'Disciples of Christ','BX7301..BX7343'),
 (12,'Dowieism','BX7401..BX7430'),
 (13,'Evangelical & Free Churches','BX7451..BX7599.Z'),
 (14,'Brethren Churches','BX7801..BX7843'),
 (15,'German Evangelical Synod','BX7901..BX7943'),
 (16,'Jehovah’s Witnesses','BX8525..BX8528'),
 (17,'Moravian Church','BX8551..BX8593'),
 (18,'Latter-day Saints','BX8601..BX8695'),
 (19,'Swedenborgianism','BX8701..BX8749'),
 (20,'Plymouth Brethren','BX8799..BX8809'),
 (21,'Puritanism','BX9301..BX9359'),
 (22,'Reformed & Calvinistic Churches','BX9401..BX9640'),
 (23,'Salvation Army','BX9701..BX9743'),
 (24,'Shakers','BX9751..BX9793'),
 (25,'Unitarianism','BX9801..BX9869'),
 (26,'United Brethren & United Churches','BX9875..BX9886'),
 (27,'Universalism','BX9901..BX9969');
INSERT INTO concept(concept_id,preferred_label)
SELECT b.id+n.seq,n.label FROM _prot_node n CROSS JOIN _prot_base b;
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT b.id+n.seq,4566,n.seq FROM _prot_node n CROSS JOIN _prot_base b;
INSERT INTO source_selector(concept_id,system_id,selector)
SELECT b.id+n.seq,'lcc',n.selector FROM _prot_node n CROSS JOIN _prot_base b;

-- Delete parent selectors whose starting number lies in a newly assigned
-- denomination schedule, plus obsolete overlapping umbrellas.
DELETE FROM source_selector
WHERE concept_id=4566 AND system_id='lcc' AND selector GLOB 'BX[0-9]*'
 AND (
   CAST(substr(selector,3) AS REAL) BETWEEN 5001 AND 6093.999
   OR CAST(substr(selector,3) AS REAL) BETWEEN 6101 AND 6193.999
   OR CAST(substr(selector,3) AS REAL) BETWEEN 6195 AND 6197.999
   OR CAST(substr(selector,3) AS REAL) BETWEEN 6201 AND 6495.999
   OR CAST(substr(selector,3) AS REAL) BETWEEN 6551 AND 6593.999
   OR CAST(substr(selector,3) AS REAL) BETWEEN 6651 AND 6693.999
   OR CAST(substr(selector,3) AS REAL) BETWEEN 6751 AND 6793.999
   OR CAST(substr(selector,3) AS REAL) BETWEEN 6801 AND 6843.999
   OR CAST(substr(selector,3) AS REAL) BETWEEN 6901 AND 7260.999
   OR CAST(substr(selector,3) AS REAL) BETWEEN 7301 AND 7343.999
   OR CAST(substr(selector,3) AS REAL) BETWEEN 7401 AND 7943.999
   OR CAST(substr(selector,3) AS REAL) BETWEEN 8001 AND 8495.999
   OR CAST(substr(selector,3) AS REAL) BETWEEN 8525 AND 8749.999
   OR CAST(substr(selector,3) AS REAL) BETWEEN 8762 AND 8787.999
   OR CAST(substr(selector,3) AS REAL) BETWEEN 8799 AND 8809.999
   OR CAST(substr(selector,3) AS REAL) BETWEEN 8901 AND 9225.999
   OR CAST(substr(selector,3) AS REAL) BETWEEN 9301 AND 9359.999
   OR CAST(substr(selector,3) AS REAL) BETWEEN 9401 AND 9640.999
   OR CAST(substr(selector,3) AS REAL) BETWEEN 9701 AND 9969.999
 );

DROP TABLE _prot_node;
DROP TABLE _prot_base;
COMMIT;

-- Compact Ancient Italy (DG11-DG365) into thematic and chronological leaves.
PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;

-- Replace legacy enumerations and overlapping era spans.
DELETE FROM source_selector
WHERE system_id='lcc' AND concept_id IN
 (3571,3572,3573,10621,10120,10145,10192,10204,10217,10377,10762,11446);

UPDATE concept SET preferred_label='Late Roman Empire, 284–364'
WHERE concept_id=10762 AND preferred_label='Late Roman Empire, 284–476';

-- Existing chronological leaves.
INSERT INTO source_selector VALUES
 (10192,'lcc','DG233..DG233.9@4'),
 (10204,'lcc','DG235..DG240@4'),
 (10120,'lcc','DG241..DG253@4'),
 (10145,'lcc','DG253.5..DG269@4'),
 (10217,'lcc','DG275..DG309.3@4'),
 (10762,'lcc','DG310..DG318.99@4'),
 (11446,'lcc','DG319..DG365@4'),
 (10377,'lcc','DG222.5..DG223.7@4');

-- Exact general numbers remain on their non-leaf era concepts.
INSERT INTO source_selector VALUES
 (10621,'lcc','DG221'),(10621,'lcc','DG221.5'),(10621,'lcc','DG222'),
 (3573,'lcc','DG231'),(3573,'lcc','DG231.3'),(3573,'lcc','DG231.7'),
 (3572,'lcc','DG269.5'),(3572,'lcc','DG270'),(3572,'lcc','DG271'),
 (3572,'lcc','DG272');

CREATE TEMP TABLE _ai_base(id INTEGER NOT NULL);
INSERT INTO _ai_base SELECT COALESCE(MAX(concept_id),0) FROM concept;
CREATE TEMP TABLE _ai_node(seq INTEGER PRIMARY KEY,parent_id INTEGER,label TEXT NOT NULL,ordinal INTEGER NOT NULL);
INSERT INTO _ai_node VALUES
 (1,3571,'General & Reference',0),
 (2,3571,'Geography & Description',1),
 (3,3571,'Local History',2),
 (4,3571,'Antiquities, Civilization & Culture',3),
 (5,3571,'Ethnography',4),
 (6,3571,'History—General',5),
 (7,10621,'Other Pre-Roman Peoples',1),
 (8,3572,'Collective Biography',0),
 (9,0,'Antiquities—General',0),
 (10,0,'Preservation of Antiquities',1),
 (11,0,'Public & Political Antiquities',2),
 (12,0,'Private Antiquities',3),
 (13,0,'Agriculture & Land',4),
 (14,0,'Commerce & Industries',5),
 (15,0,'Transportation',6),
 (16,0,'Social Institutions',7),
 (17,0,'Religious Antiquities',8),
 (18,0,'Amphitheaters',9),
 (19,0,'Aqueducts',10),
 (20,0,'Art Antiquities',11);

INSERT INTO concept(concept_id,preferred_label)
SELECT b.id+n.seq,n.label FROM _ai_node n CROSS JOIN _ai_base b;
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal,display_order)
SELECT b.id+n.seq,
 CASE WHEN n.parent_id=0 THEN b.id+4 ELSE n.parent_id END,
 n.ordinal,n.ordinal
FROM _ai_node n CROSS JOIN _ai_base b;

CREATE TEMP TABLE _ai_selector(seq INTEGER,selector TEXT);
INSERT INTO _ai_selector VALUES
 (1,'DG11'),(1,'DG12'),(1,'DG12.2..DG12.3@3'),(1,'DG12.5'),
 (1,'DG13'),(1,'DG14..DG15@3'),(1,'DG16'),
 (2,'DG26.2..DG41@3'),(3,'DG51..DG70@3'),(5,'DG190'),
 (6,'DG201'),(6,'DG203..DG204@3'),(6,'DG205..DG206@3'),
 (6,'DG206.5'),(6,'DG207..DG209@3'),(6,'DG210'),(6,'DG210.5'),
 (6,'DG211'),(6,'DG213'),(6,'DG214..DG215@3'),
 (7,'DG225.A..DG225.Z@4'),(8,'DG274..DG274.3@4'),
 (9,'DG75'),(9,'DG76..DG77@4'),(9,'DG78'),(9,'DG79'),
 (10,'DG77.5'),(11,'DG81..DG89@4'),(12,'DG90..DG95@4'),
 (13,'DG105'),(14,'DG107..DG107.8@4'),(15,'DG108'),(16,'DG109'),
 (17,'DG121..DG135@4'),(18,'DG135.5'),(19,'DG135.6'),
 (20,'DG136..DG142@4');
INSERT INTO source_selector(concept_id,system_id,selector)
SELECT b.id+s.seq,'lcc',s.selector FROM _ai_selector s CROSS JOIN _ai_base b;

DROP TABLE _ai_selector;
DROP TABLE _ai_node;
DROP TABLE _ai_base;
COMMIT;

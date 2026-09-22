-- Replace enumerated RC psychotherapy codes with official direct spans.
-- Existing Group and Couples & Family concepts retain their curated names.
PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;
CREATE TEMP TABLE _pt_active(ok INTEGER PRIMARY KEY);
INSERT INTO _pt_active SELECT 1 WHERE EXISTS(SELECT 1 FROM source_selector WHERE concept_id=5448 AND system_id='lcc');
CREATE TEMP TABLE _pt_base(id INTEGER NOT NULL);
INSERT INTO _pt_base SELECT COALESCE(MAX(concept_id),0) FROM concept;
CREATE TEMP TABLE _pt_node(seq INTEGER PRIMARY KEY,label TEXT NOT NULL,ordinal INTEGER NOT NULL);
INSERT INTO _pt_node VALUES
 (1,'Periodicals. Societies. Serials',0),
 (2,'Congresses',1),
 (3,'Dictionaries and encyclopedias',2),
 (4,'General works',3),
 (5,'General special',4),
 (6,'Popular works',5),
 (7,'Differential therapeutics',6),
 (8,'Chronic mental illness',7),
 (9,'Psychotherapy for older people',8),
 (10,'Brief psychotherapy',9),
 (11,'Crisis intervention. Hospital emergency services',10),
 (12,'Interviewing in psychiatry and mental health. History taking',11),
 (13,'Psychotherapist and the patient. Mental health personnel and patient',12),
 (14,'Client-centered psychotherapy',13),
 (15,'Physical therapy',14),
 (16,'Chemotherapy',15),
 (17,'Shock therapy',16),
 (18,'Occupational therapy',17),
 (19,'Other therapies and special aspects of therapy, A-Z',18),
 (20,'Outcome assessment',19);
CREATE TEMP TABLE _pt_selector(seq INTEGER NOT NULL,selector TEXT NOT NULL);
INSERT INTO _pt_selector VALUES
 (1,'RC475'),
 (2,'RC475.5'),
 (3,'RC475.7'),
 (4,'RC480'),
 (5,'RC480.5'),
 (6,'RC480.515'),
 (7,'RC480.52'),
 (8,'RC480.53'),
 (9,'RC480.54'),
 (10,'RC480.55'),
 (11,'RC480.6'),
 (12,'RC480.7'),
 (13,'RC480.8'),
 (14,'RC481'),
 (15,'RC482'),
 (16,'RC483..RC483.5@3'),
 (17,'RC483.9..RC485.5@3'),
 (18,'RC487'),
 (19,'RC489.A..RC489.Z@3'),
 (20,'RC480.75');
INSERT INTO concept(concept_id,preferred_label) SELECT b.id+n.seq,n.label FROM _pt_node n CROSS JOIN _pt_base b JOIN _pt_active;
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal,display_order) SELECT b.id+n.seq,5448,n.ordinal,n.ordinal FROM _pt_node n CROSS JOIN _pt_base b JOIN _pt_active;
INSERT INTO source_selector(concept_id,system_id,selector) SELECT b.id+s.seq,'lcc',s.selector FROM _pt_selector s CROSS JOIN _pt_base b JOIN _pt_active;
-- The official Group Therapy span contains the more specific family/couples
-- span. Keep only its general number on Group to avoid overlapping ownership. 
INSERT OR IGNORE INTO source_selector VALUES(5452,'lcc','RC488');
INSERT OR IGNORE INTO source_selector VALUES(5451,'lcc','RC488.5..RC488.8@4');
DELETE FROM source_selector WHERE concept_id=5448 AND system_id='lcc';
DELETE FROM source_selector
WHERE concept_id=2707 AND system_id='lcc'
  AND selector IN ('RC475','RC475.5','RC475.7','RC482','RC487','RC488');
DROP TABLE _pt_selector;
DROP TABLE _pt_node;
DROP TABLE _pt_base;
DROP TABLE _pt_active;
COMMIT;

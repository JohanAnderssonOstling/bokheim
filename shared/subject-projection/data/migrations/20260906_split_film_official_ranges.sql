-- Compact Film into official PN1993-PN1999 children. Existing screenwriting,
-- biography, and general/national history destinations are reused.
PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;
CREATE TEMP TABLE _film_active(ok INTEGER PRIMARY KEY);
INSERT INTO _film_active SELECT 1 WHERE EXISTS(SELECT 1 FROM source_selector WHERE concept_id=873 AND system_id='lcc');
CREATE TEMP TABLE _film_base(id INTEGER NOT NULL);
INSERT INTO _film_base SELECT COALESCE(MAX(concept_id),0) FROM concept;
CREATE TEMP TABLE _film_node(seq INTEGER PRIMARY KEY,label TEXT NOT NULL,ordinal INTEGER NOT NULL);
INSERT INTO _film_node VALUES
 (1,'Periodicals. Societies. Serials',0),
 (2,'Yearbooks',1),
 (3,'Museums, archives, exhibitions, festivals, etc.',2),
 (4,'Dictionaries',3),
 (5,'Study and teaching',4),
 (6,'Examinations, questions, etc.',5),
 (7,'Competitions, prizes, etc.',6),
 (8,'General works',7),
 (9,'Juvenile literature',8),
 (10,'Anecdotes, facetiae, satire',9),
 (11,'General special',10),
 (12,'Relation to history',11),
 (13,'Relation to the arts',12),
 (14,'Relation to literature',13),
 (15,'Relation to ethics, etc.',14),
 (16,'Censorship',15),
 (17,'Locations',16),
 (18,'Pictures and sound. Talking pictures',17),
 (19,'Silent films',18),
 (20,'Amateur motion pictures',19),
 (21,'Other special topics, A-Z',20),
 (22,'Plays, scenarios, etc.',21),
 (23,'Miscellaneous',22),
 (24,'Special corporations, A-Z',23),
 (25,'Laws, regulations, etc.',24),
 (26,'Relation to language',25);
CREATE TEMP TABLE _film_selector(seq INTEGER NOT NULL,selector TEXT NOT NULL);
INSERT INTO _film_selector VALUES
 (1,'PN1993'),
 (2,'PN1993.3'),
 (3,'PN1993.4..PN1993.44@3'),
 (4,'PN1993.45'),
 (5,'PN1993.7..PN1993.8@3'),
 (6,'PN1993.85'),
 (7,'PN1993.9..PN1993.93@3'),
 (8,'PN1994.A52..PN1994.Z@3'),
 (9,'PN1994.5'),
 (10,'PN1994.9'),
 (11,'PN1995'),
 (12,'PN1995.2'),
 (13,'PN1995.25'),
 (14,'PN1995.3'),
 (15,'PN1995.5'),
 (16,'PN1995.6..PN1995.65@3'),
 (17,'PN1995.67.A1..PN1995.67.Z@3'),
 (18,'PN1995.7'),
 (19,'PN1995.75'),
 (20,'PN1995.8'),
 (21,'PN1995.9.A..PN1995.9.Z@3'),
 (22,'PN1997..PN1997.85@3'),
 (23,'PN1998.A1..PN1998.Z@3'),
 (24,'PN1999.A..PN1999.Z@3'),
 (25,'PN1994.A1..PN1994.A5@3'),
 (26,'PN1995.4');
INSERT INTO concept(concept_id,preferred_label) SELECT b.id+n.seq,n.label FROM _film_node n CROSS JOIN _film_base b JOIN _film_active;
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal,display_order) SELECT b.id+n.seq,873,n.ordinal,n.ordinal FROM _film_node n CROSS JOIN _film_base b JOIN _film_active;
INSERT INTO source_selector(concept_id,system_id,selector) SELECT b.id+s.seq,'lcc',s.selector FROM _film_selector s CROSS JOIN _film_base b JOIN _film_active;
-- Reuse curated children for the three official branches omitted above.
INSERT OR IGNORE INTO source_selector VALUES(887,'lcc','PN1996');
INSERT OR IGNORE INTO source_selector VALUES(12962,'lcc','PN1993.5.A1');
INSERT OR IGNORE INTO source_selector VALUES(12963,'lcc','PN1993.6');
INSERT OR IGNORE INTO source_selector(concept_id,system_id,selector)
SELECT 12963,'lcc',selector FROM source_selector
WHERE concept_id=873 AND system_id='lcc'
  AND selector LIKE 'PN1993.5.%' AND selector<>'PN1993.5.A1';
INSERT OR IGNORE INTO source_selector VALUES(12964,'lcc','PN1998.2');
INSERT OR IGNORE INTO source_selector VALUES(12965,'lcc','PN1998.3');
-- Preserve two observed exact numbers not represented by a direct span.
DELETE FROM source_selector WHERE concept_id=873 AND system_id='lcc';
INSERT OR IGNORE INTO source_selector VALUES(873,'lcc','PN1994');
INSERT OR IGNORE INTO source_selector VALUES(873,'lcc','PN1995.85');
DROP TABLE _film_selector;
DROP TABLE _film_node;
DROP TABLE _film_base;
DROP TABLE _film_active;
COMMIT;

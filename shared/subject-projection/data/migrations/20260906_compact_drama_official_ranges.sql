-- Replace the enumerated PN drama schedule with mutually exclusive official
-- divisions.  Keep PN2000+ in the existing theater subtree.
PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;

CREATE TEMP TABLE _drama_base(id INTEGER NOT NULL);
INSERT INTO _drama_base SELECT COALESCE(MAX(concept_id),0) FROM concept;
CREATE TEMP TABLE _drama_node(
  seq INTEGER PRIMARY KEY,
  label TEXT NOT NULL,
  selector TEXT NOT NULL
);
INSERT INTO _drama_node VALUES
 (1,'Drama Periodicals & Serials','PN1600..PN1609'),
 (2,'Drama Societies','PN1611..PN1619'),
 (3,'Drama Museums & Libraries','PN1620.A1..PN1620.Z'),
 (4,'Drama Collections','PN1621..PN1623'),
 (5,'Drama Dictionaries & Indexes','PN1625..PN1627'),
 (6,'Philosophy & Aesthetics of Drama','PN1631..PN1650'),
 (7,'General Works on Drama & Stage','PN1654..PN1655'),
 (8,'Dramatic Composition & Technique','PN1660..PN1697'),
 (9,'Drama Study, Criticism & Characters','PN1701..PN1711'),
 (10,'History of Drama','PN1720..PN1861.2'),
 (11,'Special Dramatic Forms','PN1864.2..PN1988.2'),
 (12,'Broadcasting, Video & Motion Pictures','PN1990..PN1999');

INSERT INTO concept(concept_id,preferred_label)
SELECT b.id+n.seq,n.label FROM _drama_node n CROSS JOIN _drama_base b;
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT b.id+n.seq,4372,n.seq FROM _drama_node n CROSS JOIN _drama_base b;
INSERT INTO source_selector(concept_id,system_id,selector)
SELECT b.id+n.seq,'lcc',n.selector FROM _drama_node n CROSS JOIN _drama_base b;

DELETE FROM source_selector WHERE concept_id=4372 AND system_id='lcc';

DROP TABLE _drama_node;
DROP TABLE _drama_base;
COMMIT;

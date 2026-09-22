-- Normalize the PR English-literature schedule into broad history, period, and
-- genre ranges instead of enumerated schedule points.
PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;

-- Flatten selectorless navigation.
UPDATE concept_parent SET parent_concept_id=3911 WHERE parent_concept_id=3918;
DELETE FROM concept WHERE concept_id=3918;

DELETE FROM source_selector WHERE system_id='lcc' AND concept_id IN
 (3911,4401,3925,3919,3930,3906,3927,3926,3920,3910,3907,3909,3923,3929,3928);
INSERT INTO source_selector VALUES
 (3911,'lcc','PR'),
 (4401,'lcc','PR161..PR236'),
 (3925,'lcc','PR401..PR488'),
 (3919,'lcc','PR1098..PR1369.23'),
 (3930,'lcc','PR500..PR614'),
 (3906,'lcc','PR621..PR744'),
 (3927,'lcc','PR750..PR890.Z'),
 (3926,'lcc','PR901..PR907.2'),
 (3920,'lcc','PR908'),
 (3910,'lcc','PR911..PR918'),
 (3907,'lcc','PR921..PR928'),
 (3909,'lcc','PR931..PR938'),
 (3923,'lcc','PR951..PR981.2'),
 (3929,'lcc','PR111..PR120'),
 (3928,'lcc','PR125..PR138.5');
UPDATE concept SET preferred_label='Old English & Anglo-Saxon Literary History' WHERE concept_id=4401;

CREATE TEMP TABLE _english_lit_base(id INTEGER NOT NULL);
INSERT INTO _english_lit_base SELECT COALESCE(MAX(concept_id),0) FROM concept;
CREATE TEMP TABLE _english_lit_node(seq INTEGER PRIMARY KEY,label TEXT,selector_a TEXT,selector_b TEXT,selector_c TEXT);
INSERT INTO _english_lit_node VALUES
 (1,'General History of English Literature','PR80.2..PR110.Z','PR121..PR124.Z','PR139.2..PR153.Z'),
 (2,'Medieval Literary History, 1066–1500','PR251..PR369.22',NULL,NULL),
 (3,'Miscellany, Curiosa & Eccentric Literature','PR941..PR948',NULL,NULL);
INSERT INTO concept(concept_id,preferred_label)
SELECT b.id+n.seq,n.label FROM _english_lit_node n CROSS JOIN _english_lit_base b;
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT b.id+n.seq,3911,
       COALESCE((SELECT MAX(ordinal)+1 FROM concept_parent WHERE parent_concept_id=3911),0)+n.seq
FROM _english_lit_node n CROSS JOIN _english_lit_base b;
INSERT INTO source_selector(concept_id,system_id,selector)
SELECT b.id+n.seq,'lcc',n.selector_a FROM _english_lit_node n CROSS JOIN _english_lit_base b;
INSERT INTO source_selector(concept_id,system_id,selector)
SELECT b.id+n.seq,'lcc',n.selector_b FROM _english_lit_node n CROSS JOIN _english_lit_base b WHERE n.selector_b IS NOT NULL;
INSERT INTO source_selector(concept_id,system_id,selector)
SELECT b.id+n.seq,'lcc',n.selector_c FROM _english_lit_node n CROSS JOIN _english_lit_base b WHERE n.selector_c IS NOT NULL;

DROP TABLE _english_lit_node;
DROP TABLE _english_lit_base;
COMMIT;

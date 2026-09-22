-- Partition International Relations into mutually exclusive official JX/JZ
-- leaves and remove the parent fallback enumeration.
PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;

-- Existing organization and diplomacy leaves: collapse overlapping ranges.
DELETE FROM source_selector WHERE concept_id=5347 AND system_id='lcc';
INSERT INTO source_selector VALUES(5347,'lcc','JX1621..JX1896');
DELETE FROM source_selector WHERE concept_id=5348 AND system_id='lcc';
INSERT INTO source_selector VALUES(5348,'lcc','JZ4835..JZ4840.Z');
DELETE FROM source_selector WHERE concept_id=5343 AND system_id='lcc';
INSERT INTO source_selector VALUES(5343,'lcc','JZ4850..JZ5490.2');

CREATE TEMP TABLE _ir_base(id INTEGER NOT NULL);
INSERT INTO _ir_base SELECT COALESCE(MAX(concept_id),0) FROM concept;
CREATE TEMP TABLE _ir_node(seq INTEGER PRIMARY KEY,label TEXT,selector1 TEXT,selector2 TEXT);
INSERT INTO _ir_node VALUES
 (1,'Historical JX Schedule','JX1..JX1620.9','JX1897..JX6650'),
 (2,'Bibliography, Periodicals & Conferences','JZ2..JZ23.9','JZ38.1..JZ62.9'),
 (3,'Research, Study & Teaching','JZ1160..JZ1248.9',NULL),
 (4,'Theory of the Law of Nations','JZ1256..JZ1304.9',NULL),
 (5,'Publicists & Writers on International Law','JZ2064..JZ3669',NULL),
 (6,'The International Community','JZ3900..JZ4059',NULL),
 (7,'Peace, Security & Disarmament','JZ5509.2..JZ6300',NULL),
 (8,'Law of War & Humanitarian Law','JZ6440..JZ6530',NULL);
INSERT INTO concept(concept_id,preferred_label)
SELECT b.id+n.seq,n.label FROM _ir_node n CROSS JOIN _ir_base b;
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT b.id+n.seq,5693,n.seq FROM _ir_node n CROSS JOIN _ir_base b;
INSERT INTO source_selector(concept_id,system_id,selector)
SELECT b.id+n.seq,'lcc',n.selector1 FROM _ir_node n CROSS JOIN _ir_base b
UNION ALL
SELECT b.id+n.seq,'lcc',n.selector2 FROM _ir_node n CROSS JOIN _ir_base b
WHERE n.selector2 IS NOT NULL;

DELETE FROM source_selector WHERE concept_id=5693 AND system_id='lcc';
INSERT INTO source_selector VALUES(5693,'lcc','JZ');

DROP TABLE _ir_node;
DROP TABLE _ir_base;
COMMIT;

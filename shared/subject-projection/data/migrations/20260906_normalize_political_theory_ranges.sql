-- Partition JC political theory into mutually exclusive official leaves.
PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;

DELETE FROM source_selector WHERE concept_id=12526 AND system_id='lcc';
INSERT INTO source_selector VALUES(12526,'lcc','JC51..JC93');
DELETE FROM source_selector WHERE concept_id=12527 AND system_id='lcc';
INSERT INTO source_selector VALUES(12527,'lcc','JC109..JC128.2');
DELETE FROM source_selector WHERE concept_id=12528 AND system_id='lcc';
INSERT INTO source_selector VALUES(12528,'lcc','JC131..JC275');
DELETE FROM source_selector WHERE concept_id=12529 AND system_id='lcc';
INSERT INTO source_selector VALUES(12529,'lcc','JC319..JC323');
DELETE FROM source_selector WHERE concept_id=12530 AND system_id='lcc';
INSERT INTO source_selector VALUES(12530,'lcc','JC489..JC497');
DELETE FROM source_selector WHERE concept_id=12531 AND system_id='lcc';
INSERT INTO source_selector VALUES(12531,'lcc','JC571..JC607');

CREATE TEMP TABLE _jc_base(id INTEGER NOT NULL);
INSERT INTO _jc_base SELECT COALESCE(MAX(concept_id),0) FROM concept;
CREATE TEMP TABLE _jc_node(seq INTEGER PRIMARY KEY,label TEXT,selector1 TEXT,selector2 TEXT);
INSERT INTO _jc_node VALUES
 (1,'Political Theory—General','JC11..JC20.9','JC31.1..JC50.9'),
 (2,'Political Anthropology','JC21..JC31',NULL),
 (3,'Nationalism & the Nation-State','JC311..JC314',NULL),
 (4,'Sovereignty, Power & Theories of the State','JC327..JC347',NULL),
 (5,'Forms of the State','JC348..JC488.9',NULL),
 (6,'Purpose & Functions of the State','JC501..JC570.9',NULL);
INSERT INTO concept(concept_id,preferred_label)
SELECT b.id+n.seq,n.label FROM _jc_node n CROSS JOIN _jc_base b;
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT b.id+n.seq,5370,n.seq FROM _jc_node n CROSS JOIN _jc_base b;
INSERT INTO source_selector(concept_id,system_id,selector)
SELECT b.id+n.seq,'lcc',n.selector1 FROM _jc_node n CROSS JOIN _jc_base b
UNION ALL
SELECT b.id+n.seq,'lcc',n.selector2 FROM _jc_node n CROSS JOIN _jc_base b
WHERE n.selector2 IS NOT NULL;

DELETE FROM source_selector WHERE concept_id=5370 AND system_id='lcc';
INSERT INTO source_selector VALUES(5370,'lcc','JC');

DROP TABLE _jc_node;
DROP TABLE _jc_base;
COMMIT;

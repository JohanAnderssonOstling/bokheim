-- Compact the remaining QA general/reference enumeration around the existing
-- philosophy, history, biography, early-works and general-works children.
PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;

DELETE FROM source_selector WHERE concept_id=4969 AND system_id='lcc';
INSERT INTO source_selector VALUES(4969,'lcc','QA10.92..QA20.3');

CREATE TEMP TABLE _mathref_base(id INTEGER NOT NULL);
INSERT INTO _mathref_base SELECT COALESCE(MAX(concept_id),0) FROM concept;
CREATE TEMP TABLE _mathref_node(seq INTEGER PRIMARY KEY,label TEXT,selector_a TEXT,selector_b TEXT);
INSERT INTO _mathref_node VALUES
 (1,'General Reference','QA1..QA6.Z','QA40..QA41.3'),
 (2,'Profession & Social Aspects','QA10.5..QA10.8',NULL);
INSERT INTO concept(concept_id,preferred_label)
SELECT b.id+n.seq,n.label FROM _mathref_node n CROSS JOIN _mathref_base b;
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT b.id+n.seq,5722,
       COALESCE((SELECT MAX(ordinal)+1 FROM concept_parent WHERE parent_concept_id=5722),0)+n.seq-1
FROM _mathref_node n CROSS JOIN _mathref_base b;
INSERT INTO source_selector(concept_id,system_id,selector)
SELECT b.id+n.seq,'lcc',n.selector_a FROM _mathref_node n CROSS JOIN _mathref_base b;
INSERT INTO source_selector(concept_id,system_id,selector)
SELECT b.id+n.seq,'lcc',n.selector_b FROM _mathref_node n CROSS JOIN _mathref_base b
WHERE n.selector_b IS NOT NULL;

DELETE FROM source_selector WHERE concept_id=5722 AND system_id='lcc';
INSERT INTO source_selector VALUES(5722,'lcc','QA30');
INSERT INTO source_selector VALUES(5722,'lcc','QA43');

DROP TABLE _mathref_node;
DROP TABLE _mathref_base;
COMMIT;

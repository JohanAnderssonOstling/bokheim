-- Replace American literature's PS700-PS3626 author umbrella and general
-- history enumeration with disjoint period and literary-history leaves.
PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;

DELETE FROM source_selector WHERE concept_id=3860 AND system_id='lcc';
INSERT INTO source_selector VALUES(3860,'lcc','PS360..PS380');
DELETE FROM source_selector WHERE concept_id=3858 AND system_id='lcc';
INSERT INTO source_selector VALUES(3858,'lcc','PS451..PS478');

INSERT OR IGNORE INTO source_selector VALUES(12720,'lcc','PS3600..PS3626');

CREATE TEMP TABLE _amlit_base(id INTEGER NOT NULL);
INSERT INTO _amlit_base SELECT COALESCE(MAX(concept_id),0) FROM concept;
CREATE TEMP TABLE _amlit_node(seq INTEGER PRIMARY KEY,label TEXT,selector_a TEXT,selector_b TEXT,selector_c TEXT);
INSERT INTO _amlit_node VALUES
 (1,'General Literary History','PS84.2..PS229',NULL,NULL),
 (2,'Regional Literary History','PS538..PS549','PS551..PS559','PS561..PS572'),
 (3,'Colonial Period, 17th–18th Centuries','PS700..PS893',NULL,NULL),
 (4,'19th Century','PS991..PS3390',NULL,NULL);
INSERT INTO concept(concept_id,preferred_label)
SELECT b.id+n.seq,n.label FROM _amlit_node n CROSS JOIN _amlit_base b;
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT b.id+n.seq,3853,
       COALESCE((SELECT MAX(ordinal)+1 FROM concept_parent WHERE parent_concept_id=3853),0)+n.seq-1
FROM _amlit_node n CROSS JOIN _amlit_base b;
INSERT INTO source_selector(concept_id,system_id,selector)
SELECT b.id+n.seq,'lcc',n.selector_a FROM _amlit_node n CROSS JOIN _amlit_base b;
INSERT INTO source_selector(concept_id,system_id,selector)
SELECT b.id+n.seq,'lcc',n.selector_b FROM _amlit_node n CROSS JOIN _amlit_base b WHERE n.selector_b IS NOT NULL;
INSERT INTO source_selector(concept_id,system_id,selector)
SELECT b.id+n.seq,'lcc',n.selector_c FROM _amlit_node n CROSS JOIN _amlit_base b WHERE n.selector_c IS NOT NULL;

DELETE FROM source_selector WHERE concept_id=3853 AND system_id='lcc';

DROP TABLE _amlit_node;
DROP TABLE _amlit_base;
COMMIT;

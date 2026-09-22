-- Move Georgian and Azerbaijani schedules out of the Eastern Europe umbrella.
PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;

DELETE FROM source_selector
WHERE concept_id=3522 AND system_id='lcc'
  AND CAST(substr(selector,3) AS REAL) BETWEEN 670 AND 699.999;

CREATE TEMP TABLE _caucasus_base(id INTEGER NOT NULL);
INSERT INTO _caucasus_base SELECT COALESCE(MAX(concept_id),0) FROM concept;
CREATE TEMP TABLE _caucasus_node(seq INTEGER PRIMARY KEY,parent_id INTEGER,label TEXT,selector_a TEXT,selector_b TEXT);
INSERT INTO _caucasus_node VALUES
 (1,3589,'General & Reference','DK670..DK677.09','DK678.31..DK678.99'),
 (2,3589,'Regions & Cities','DK679..DK679.5.Z',NULL),
 (3,3586,'General & Reference','DK690..DK697.09','DK697.81..DK698.99'),
 (4,3586,'Regions & Cities','DK699..DK699.5.Z',NULL);
INSERT INTO concept(concept_id,preferred_label)
SELECT b.id+n.seq,n.label FROM _caucasus_node n CROSS JOIN _caucasus_base b;
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT b.id+n.seq,n.parent_id,
       COALESCE((SELECT MAX(ordinal)+1 FROM concept_parent WHERE parent_concept_id=n.parent_id),0)+n.seq
FROM _caucasus_node n CROSS JOIN _caucasus_base b;
INSERT INTO source_selector(concept_id,system_id,selector)
SELECT b.id+n.seq,'lcc',n.selector_a FROM _caucasus_node n CROSS JOIN _caucasus_base b;
INSERT INTO source_selector(concept_id,system_id,selector)
SELECT b.id+n.seq,'lcc',n.selector_b FROM _caucasus_node n CROSS JOIN _caucasus_base b WHERE n.selector_b IS NOT NULL;

DROP TABLE _caucasus_node;
DROP TABLE _caucasus_base;
COMMIT;

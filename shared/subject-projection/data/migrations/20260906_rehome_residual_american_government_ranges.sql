-- Separate the JF general/comparative schedule from residual JK United States
-- codes. The latter belong under the existing American Government branch.
PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;

DELETE FROM source_selector
WHERE concept_id=5366 AND system_id='lcc'
  AND (selector GLOB 'JF*' OR selector GLOB 'JK*');
INSERT INTO source_selector VALUES(5366,'lcc','JF');
INSERT INTO source_selector VALUES(5366,'lcc','JF20..JF2112');

CREATE TEMP TABLE _usgov_base(id INTEGER NOT NULL);
INSERT INTO _usgov_base SELECT COALESCE(MAX(concept_id),0) FROM concept;
CREATE TEMP TABLE _usgov_node(seq INTEGER PRIMARY KEY,label TEXT,selector TEXT);
INSERT INTO _usgov_node VALUES
 (1,'General & Historical American Government','JK1..JK400'),
 (2,'Confederate States Government','JK9663..JK9993');
INSERT INTO concept(concept_id,preferred_label)
SELECT b.id+n.seq,n.label FROM _usgov_node n CROSS JOIN _usgov_base b;
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT b.id+n.seq,5327,
       COALESCE((SELECT MAX(ordinal)+1 FROM concept_parent WHERE parent_concept_id=5327),0)+n.seq-1
FROM _usgov_node n CROSS JOIN _usgov_base b;
INSERT INTO source_selector(concept_id,system_id,selector)
SELECT b.id+n.seq,'lcc',n.selector FROM _usgov_node n CROSS JOIN _usgov_base b;

DROP TABLE _usgov_node;
DROP TABLE _usgov_base;
COMMIT;

-- Replace American literature Cutter enumerations and parent ranges with leaves.
PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;

CREATE TEMP TABLE _am_base(id INTEGER NOT NULL);
INSERT INTO _am_base SELECT COALESCE(MAX(concept_id),0) FROM concept;
CREATE TEMP TABLE _am_node(seq INTEGER PRIMARY KEY,label TEXT NOT NULL,selector TEXT NOT NULL);
INSERT INTO _am_node VALUES
 (1,'American Literature—General & Reference','PS1..PS54'),
 (2,'American Criticism by Period','PS80..PS96'),
 (3,'Special Subjects in American Literature','PS169.A..PS169.Z'),
 (4,'Special Classes in American Literature','PS173.A..PS173.Z'),
 (5,'American Drama—Special Topics','PS338.A..PS338.Z');

INSERT INTO concept(concept_id,preferred_label)
SELECT b.id+n.seq,n.label FROM _am_node n CROSS JOIN _am_base b;
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT b.id+n.seq,4311,0 FROM _am_node n CROSS JOIN _am_base b;
INSERT INTO source_selector(concept_id,system_id,selector)
SELECT b.id+n.seq,'lcc',n.selector FROM _am_node n CROSS JOIN _am_base b;

DELETE FROM source_selector
WHERE concept_id=4311 AND system_id='lcc' AND (
 selector='PS1..PS54' OR selector LIKE 'PS169.%' OR selector LIKE 'PS173.%'
 OR selector LIKE 'PS338.%' OR selector IN ('PS80','PS81','PS85','PS88','PS92','PS94','PS96')
);

DROP TABLE _am_node;
DROP TABLE _am_base;
COMMIT;

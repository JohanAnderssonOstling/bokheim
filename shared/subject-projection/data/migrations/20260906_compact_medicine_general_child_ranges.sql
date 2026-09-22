-- Move rangeable Medicine (General) blocks from the nonleaf into ranged leaves.
PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;

CREATE TEMP TABLE _med_base(id INTEGER NOT NULL);
INSERT INTO _med_base SELECT COALESCE(MAX(concept_id),0) FROM concept;
CREATE TEMP TABLE _med_node(seq INTEGER PRIMARY KEY,label TEXT NOT NULL,selector TEXT NOT NULL);
INSERT INTO _med_node VALUES
 (1,'General Medical Reference','R5..R130.5'),
 (2,'Physicians’ Personal Life','R707..R707.4'),
 (3,'Medical Directories','R711..R713.97'),
 (4,'Medical Laboratories & Institutes','R860..R862'),
 (5,'Medical Museums & Exhibitions','R871..R891');

INSERT INTO concept(concept_id,preferred_label)
SELECT b.id+n.seq,n.label FROM _med_node n CROSS JOIN _med_base b;
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT b.id+n.seq,2725,0 FROM _med_node n CROSS JOIN _med_base b;
INSERT INTO source_selector(concept_id,system_id,selector)
SELECT b.id+n.seq,'lcc',n.selector FROM _med_node n CROSS JOIN _med_base b;

DELETE FROM source_selector
WHERE concept_id=2725 AND system_id='lcc' AND (
 selector='R5..R130.5' OR selector GLOB 'R707*' OR selector GLOB 'R711*'
 OR selector GLOB 'R712*' OR selector GLOB 'R713*'
 OR selector IN ('R860','R871','R881','R131-684','R723-723.7','R856-857','R895-920')
);

DROP TABLE _med_node;
DROP TABLE _med_base;
COMMIT;

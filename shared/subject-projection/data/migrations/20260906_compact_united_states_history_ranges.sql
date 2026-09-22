-- Restore mutually exclusive ranged ownership beneath United States History.
PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;

-- Expand the existing specific population leaf to its official schedule span.
DELETE FROM source_selector WHERE concept_id=3378 AND system_id='lcc';
INSERT INTO source_selector VALUES(3378,'lcc','E184.4..E185.98');

CREATE TEMP TABLE _ush_base(id INTEGER NOT NULL);
INSERT INTO _ush_base SELECT COALESCE(MAX(concept_id),0) FROM concept;
CREATE TEMP TABLE _ush_node(seq INTEGER PRIMARY KEY,label TEXT NOT NULL,selector TEXT NOT NULL);
INSERT INTO _ush_node VALUES
 (1,'United States—General','E151..E169.13'),
 (2,'United States Historical Reference','E171..E179.52'),
 (3,'Americans Abroad','E184.2'),
 (4,'Jewish Americans','E184.3..E184.37'),
 (5,'Other Population Groups','E184.A..E184.Z');
INSERT INTO concept(concept_id,preferred_label)
SELECT b.id+n.seq,n.label FROM _ush_node n CROSS JOIN _ush_base b;
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT b.id+n.seq,3402,0 FROM _ush_node n CROSS JOIN _ush_base b;
INSERT INTO source_selector(concept_id,system_id,selector)
SELECT b.id+n.seq,'lcc',n.selector FROM _ush_node n CROSS JOIN _ush_base b;

-- Remove the general/population enumerations and ancestor copies already owned
-- by Colonial, Civil War, twentieth-century, and twenty-first-century leaves.
DELETE FROM source_selector WHERE concept_id=3402 AND system_id='lcc' AND (
 selector='F1..F975' OR selector GLOB 'E15*' OR selector GLOB 'E16*'
 OR selector GLOB 'E17*' OR selector GLOB 'E184*' OR selector GLOB 'E185*'
 OR selector GLOB 'E186*' OR selector GLOB 'E4*' OR selector GLOB 'E5*'
 OR selector GLOB 'E6*' OR selector GLOB 'E78*' OR selector GLOB 'E89*'
);

-- Exact codes already owned by existing thematic children.
DELETE FROM source_selector
WHERE concept_id=3402 AND system_id='lcc' AND selector='E182';

DROP TABLE _ush_node;
DROP TABLE _ush_base;
COMMIT;

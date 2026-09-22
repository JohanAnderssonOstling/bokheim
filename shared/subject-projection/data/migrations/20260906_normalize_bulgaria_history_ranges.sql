-- Normalize Bulgaria to the official DR51-DR98 schedule blocks.
PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;

CREATE TEMP TABLE _bulgaria_base(id INTEGER NOT NULL);
INSERT INTO _bulgaria_base SELECT COALESCE(MAX(concept_id),0) FROM concept;

INSERT INTO concept(concept_id,preferred_label)
SELECT id+1,'Bulgaria' FROM _bulgaria_base;
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT id+1,2455,COALESCE((SELECT MAX(ordinal)+1 FROM concept_parent WHERE parent_concept_id=2455),0)
FROM _bulgaria_base;
INSERT INTO source_selector SELECT id+1,'lcc','DR57..DR60.2' FROM _bulgaria_base;

DELETE FROM source_selector
WHERE system_id='lcc' AND concept_id IN
 (3545,11500,11501,11502,11503,11504,11505,11506,11499,13763,13764);
INSERT INTO source_selector VALUES
 (11504,'lcc','DR74.3'),
 (11502,'lcc','DR74.5..DR75.8'),
 (11503,'lcc','DR77..DR77.8'),
 (11505,'lcc','DR80..DR80.8'),
 (11506,'lcc','DR81..DR84'),
 (11499,'lcc','DR84.9..DR89.8'),
 (11500,'lcc','DR89.9..DR93.34'),
 (11501,'lcc','DR93.4..DR93.47'),
 (13763,'lcc','DR95.A..DR95.Z'),
 (13764,'lcc','DR97..DR98.Z');

CREATE TEMP TABLE _bulgaria_node(seq INTEGER PRIMARY KEY,label TEXT,selector_a TEXT,selector_b TEXT);
INSERT INTO _bulgaria_node VALUES
 (2,'General & Reference','DR51..DR56.7','DR61..DR64.2.Z'),
 (3,'General Historical Studies','DR65..DR73.Z',NULL),
 (4,'Greek Rule, 1018–1185','DR79..DR79.25',NULL);
INSERT INTO concept(concept_id,preferred_label)
SELECT b.id+n.seq,n.label FROM _bulgaria_node n CROSS JOIN _bulgaria_base b;
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT b.id+n.seq,3545,
       COALESCE((SELECT MAX(ordinal)+1 FROM concept_parent WHERE parent_concept_id=3545),0)+n.seq-2
FROM _bulgaria_node n CROSS JOIN _bulgaria_base b;
INSERT INTO source_selector(concept_id,system_id,selector)
SELECT b.id+n.seq,'lcc',n.selector_a FROM _bulgaria_node n CROSS JOIN _bulgaria_base b;
INSERT INTO source_selector(concept_id,system_id,selector)
SELECT b.id+n.seq,'lcc',n.selector_b FROM _bulgaria_node n CROSS JOIN _bulgaria_base b
WHERE n.selector_b IS NOT NULL;

DROP TABLE _bulgaria_node;
DROP TABLE _bulgaria_base;
COMMIT;

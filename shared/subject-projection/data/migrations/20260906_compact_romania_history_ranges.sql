-- Replace Romania's long selector list with mutually exclusive broad LCC spans.
PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;

-- Description and travel belongs in the travel branch, not history.
CREATE TEMP TABLE _romania_base(id INTEGER NOT NULL);
INSERT INTO _romania_base SELECT COALESCE(MAX(concept_id),0) FROM concept;
INSERT INTO concept(concept_id,preferred_label)
SELECT id+1,'Romania' FROM _romania_base;
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT id+1,2455,COALESCE((SELECT MAX(ordinal)+1 FROM concept_parent WHERE parent_concept_id=2455),0)
FROM _romania_base;
INSERT INTO source_selector
SELECT id+1,'lcc','DR207..DR210' FROM _romania_base;

-- Delete the overlapping umbrella; its schedule is represented by the adjacent
-- earliest/Roman and medieval leaves.
DELETE FROM concept WHERE concept_id=11550;

DELETE FROM source_selector
WHERE system_id='lcc' AND concept_id IN
 (11539,11540,11541,11542,11543,11544,11545,11546,11547,11548,11549,11551,11552,13762);
INSERT INTO source_selector VALUES
 (11548,'lcc','DR238..DR239.22'),
 (11551,'lcc','DR240..DR240.5'),
 (11552,'lcc','DR241..DR241.5'),
 (11540,'lcc','DR242..DR244.7'),
 (11541,'lcc','DR246'),
 (11543,'lcc','DR248..DR249'),
 (11542,'lcc','DR250..DR263.Z'),
 (11545,'lcc','DR264..DR266.5'),
 (11546,'lcc','DR267..DR267.5'),
 (11547,'lcc','DR268..DR269.6'),
 (11549,'lcc','DR280.2..DR280.26'),
 (11539,'lcc','DR280.4..DR280.44'),
 (11544,'lcc','DR280.7..DR280.74'),
 (13762,'lcc','DR296.A..DR296.Z');

CREATE TEMP TABLE _romania_node(seq INTEGER PRIMARY KEY,label TEXT,selector_a TEXT,selector_b TEXT,selector_c TEXT);
INSERT INTO _romania_node VALUES
 (2,'General & Reference','DR201..DR206.9','DR211..DR214.2',NULL),
 (3,'General Historical Studies','DR215..DR237.Z',NULL,NULL),
 (4,'General Transylvania','DR279..DR280.19','DR280.27..DR280.39','DR280.45..DR280.69'),
 (5,'Regions & Major Cities','DR281..DR295.Z',NULL,NULL);
INSERT INTO concept(concept_id,preferred_label)
SELECT b.id+n.seq,n.label FROM _romania_node n CROSS JOIN _romania_base b;
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT b.id+n.seq,3548,
       COALESCE((SELECT MAX(ordinal)+1 FROM concept_parent WHERE parent_concept_id=3548),0)+n.seq-2
FROM _romania_node n CROSS JOIN _romania_base b;
INSERT INTO source_selector(concept_id,system_id,selector)
SELECT b.id+n.seq,'lcc',n.selector_a FROM _romania_node n CROSS JOIN _romania_base b;
INSERT INTO source_selector(concept_id,system_id,selector)
SELECT b.id+n.seq,'lcc',n.selector_b FROM _romania_node n CROSS JOIN _romania_base b WHERE n.selector_b IS NOT NULL;
INSERT INTO source_selector(concept_id,system_id,selector)
SELECT b.id+n.seq,'lcc',n.selector_c FROM _romania_node n CROSS JOIN _romania_base b WHERE n.selector_c IS NOT NULL;

-- DR23 belongs to the Balkan-general schedule and was contamination here.
DELETE FROM source_selector WHERE concept_id=3548 AND system_id='lcc';

DROP TABLE _romania_node;
DROP TABLE _romania_base;
COMMIT;

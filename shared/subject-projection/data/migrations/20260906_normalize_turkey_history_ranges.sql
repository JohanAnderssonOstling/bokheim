-- Normalize Turkey to mutually exclusive broad DR schedule ranges.
PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;

-- Move the official description-and-travel span to the existing travel leaf.
INSERT OR IGNORE INTO source_selector VALUES(2488,'lcc','DR421..DR429.4');

DELETE FROM source_selector
WHERE system_id='lcc' AND concept_id IN
 (3551,11400,11399,11563,11565,11564,11561,11562,11401);
INSERT INTO source_selector VALUES
 (11400,'lcc','DR481'),
 (11399,'lcc','DR485..DR510.Z'),
 (11563,'lcc','DR511..DR529'),
 (11565,'lcc','DR531..DR555.7'),
 (11564,'lcc','DR556..DR564'),
 (11561,'lcc','DR565..DR567'),
 (11562,'lcc','DR568..DR575'),
 (11401,'lcc','DR576..DR605.Z');
UPDATE concept SET preferred_label='Early Ottoman Empire, 1281/1453–1566'
WHERE concept_id=11399;

CREATE TEMP TABLE _turkey_base(id INTEGER NOT NULL);
INSERT INTO _turkey_base SELECT COALESCE(MAX(concept_id),0) FROM concept;
CREATE TEMP TABLE _turkey_node(seq INTEGER PRIMARY KEY,label TEXT,selector_a TEXT,selector_b TEXT);
INSERT INTO _turkey_node VALUES
 (1,'General & Reference','DR401..DR420.Z',NULL),
 (2,'Antiquities, Culture & Ethnography','DR431..DR435.Z',NULL),
 (3,'General Historical Studies','DR436..DR480.Z',NULL),
 (4,'Istanbul','DR716..DR739',NULL),
 (5,'Other European Turkey','DR701..DR715.Z','DR740..DR741.Z');
INSERT INTO concept(concept_id,preferred_label)
SELECT b.id+n.seq,n.label FROM _turkey_node n CROSS JOIN _turkey_base b;
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT b.id+n.seq,3551,
       COALESCE((SELECT MAX(ordinal)+1 FROM concept_parent WHERE parent_concept_id=3551),0)+n.seq-1
FROM _turkey_node n CROSS JOIN _turkey_base b;
INSERT INTO source_selector(concept_id,system_id,selector)
SELECT b.id+n.seq,'lcc',n.selector_a FROM _turkey_node n CROSS JOIN _turkey_base b;
INSERT INTO source_selector(concept_id,system_id,selector)
SELECT b.id+n.seq,'lcc',n.selector_b FROM _turkey_node n CROSS JOIN _turkey_base b
WHERE n.selector_b IS NOT NULL;

DROP TABLE _turkey_node;
DROP TABLE _turkey_base;
COMMIT;

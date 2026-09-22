-- Remove the all-DR umbrella and express the Balkan-general schedule as broad,
-- mutually exclusive ranges. Country schedules remain with their country leaves.
PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;

CREATE TEMP TABLE _balkan_base(id INTEGER NOT NULL);
INSERT INTO _balkan_base SELECT COALESCE(MAX(concept_id),0) FROM concept;

-- Description and travel belongs under Travel > Europe.
INSERT INTO concept(concept_id,preferred_label)
SELECT id+1,'Balkan Peninsula' FROM _balkan_base;
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT id+1,2455,COALESCE((SELECT MAX(ordinal)+1 FROM concept_parent WHERE parent_concept_id=2455),0)
FROM _balkan_base;
INSERT INTO source_selector
SELECT id+1,'lcc','DR11.5..DR16' FROM _balkan_base;

-- Flatten the selectorless "By period" navigation layer.
UPDATE concept_parent SET parent_concept_id=3541
WHERE parent_concept_id=11439 AND concept_id IN (10106,10195,10288);
DELETE FROM concept WHERE concept_id IN (3542,3543,7717,3547,11439);

DELETE FROM source_selector
WHERE system_id='lcc' AND concept_id IN (3541,10106,10195,10288,3550);
INSERT INTO source_selector VALUES
 (3541,'lcc','DR'),
 (10195,'lcc','DR39..DR40.Z'),
 (10288,'lcc','DR41..DR44.Z'),
 (10106,'lcc','DR45..DR48.9'),
 (3550,'lcc','DR50..DR50.84');
UPDATE concept SET preferred_label='Balkan Peninsula, 19th Century'
WHERE concept_id=10288;

CREATE TEMP TABLE _balkan_node(seq INTEGER PRIMARY KEY,label TEXT,selector_a TEXT,selector_b TEXT);
INSERT INTO _balkan_node VALUES
 (2,'General & Reference','DR1..DR10','DR17..DR23.Z'),
 (3,'Ethnography','DR24..DR27.Z',NULL),
 (4,'General Historical Studies','DR32..DR38.3.Z',NULL),
 (5,'Lower Danube River Valley','DR49..DR49.26',NULL);
INSERT INTO concept(concept_id,preferred_label)
SELECT b.id+n.seq,n.label FROM _balkan_node n CROSS JOIN _balkan_base b;
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT b.id+n.seq,3541,
       COALESCE((SELECT MAX(ordinal)+1 FROM concept_parent WHERE parent_concept_id=3541),0)+n.seq-2
FROM _balkan_node n CROSS JOIN _balkan_base b;
INSERT INTO source_selector(concept_id,system_id,selector)
SELECT b.id+n.seq,'lcc',n.selector_a FROM _balkan_node n CROSS JOIN _balkan_base b;
INSERT INTO source_selector(concept_id,system_id,selector)
SELECT b.id+n.seq,'lcc',n.selector_b FROM _balkan_node n CROSS JOIN _balkan_base b
WHERE n.selector_b IS NOT NULL;

DROP TABLE _balkan_node;
DROP TABLE _balkan_base;
COMMIT;

-- Distribute the PN2000-PN3307 theater schedule among terminal children.
-- Only three isolated special-topic codes remain on the nonleaf parent.
PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;

DELETE FROM source_selector WHERE concept_id IN (12508,12509,12510) AND system_id='lcc';
INSERT INTO source_selector VALUES(12508,'lcc','PN2061..PN2084');
INSERT INTO source_selector VALUES(12509,'lcc','PN2085..PN2093');
INSERT INTO source_selector VALUES(12510,'lcc','PN2100..PN2193');

-- Carve the broad other-regions span around the five named country children
-- so every call number has one owner.
DELETE FROM source_selector WHERE concept_id=12674 AND system_id='lcc';
INSERT INTO source_selector VALUES(12674,'lcc','PN2219.3..PN2219.51');
INSERT INTO source_selector VALUES(12674,'lcc','PN2299..PN2574.Z');
INSERT INTO source_selector VALUES(12674,'lcc','PN2610..PN2619.Z');
INSERT INTO source_selector VALUES(12674,'lcc','PN2660..PN2669.Z');
INSERT INTO source_selector VALUES(12674,'lcc','PN2690..PN3030');

CREATE TEMP TABLE _theater_base(id INTEGER NOT NULL);
INSERT INTO _theater_base SELECT COALESCE(MAX(concept_id),0) FROM concept;
CREATE TEMP TABLE _theater_node(seq INTEGER PRIMARY KEY,label TEXT,selector_a TEXT,selector_b TEXT);
INSERT INTO _theater_node VALUES
 (1,'General, Reference & Theater Culture','PN2000..PN2060.Z','PN2205..PN2217'),
 (2,'School & College Theater','PN3175..PN3191',NULL),
 (3,'Applied & Community Theater','PN3302..PN3307',NULL);
INSERT INTO concept(concept_id,preferred_label)
SELECT b.id+n.seq,n.label FROM _theater_node n CROSS JOIN _theater_base b;
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT b.id+n.seq,4373,
       COALESCE((SELECT MAX(ordinal)+1 FROM concept_parent WHERE parent_concept_id=4373),0)+n.seq-1
FROM _theater_node n CROSS JOIN _theater_base b;
INSERT INTO source_selector(concept_id,system_id,selector)
SELECT b.id+n.seq,'lcc',n.selector_a FROM _theater_node n CROSS JOIN _theater_base b;
INSERT INTO source_selector(concept_id,system_id,selector)
SELECT b.id+n.seq,'lcc',n.selector_b FROM _theater_node n CROSS JOIN _theater_base b
WHERE n.selector_b IS NOT NULL;

DELETE FROM source_selector WHERE concept_id=4373 AND system_id='lcc';
INSERT INTO source_selector VALUES(4373,'lcc','PN3032');
INSERT INTO source_selector VALUES(4373,'lcc','PN3035');
INSERT INTO source_selector VALUES(4373,'lcc','PN3195');

DROP TABLE _theater_node;
DROP TABLE _theater_base;
COMMIT;

-- Replace Anglo-Saxon literature's parent enumeration with official leaves.
PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;

CREATE TEMP TABLE _oe_base(id INTEGER NOT NULL);
INSERT INTO _oe_base SELECT COALESCE(MAX(concept_id),0) FROM concept;
INSERT INTO concept(concept_id,preferred_label) SELECT id+1,'Old English Collections' FROM _oe_base;
INSERT INTO concept(concept_id,preferred_label) SELECT id+2,'Old English Authors & Works' FROM _oe_base;
INSERT INTO concept(concept_id,preferred_label) SELECT id+3,'Latin Literature of the Anglo-Saxon Period' FROM _oe_base;
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal) SELECT id+1,4401,0 FROM _oe_base;
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal) SELECT id+2,4401,0 FROM _oe_base;
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal) SELECT id+3,4401,0 FROM _oe_base;
INSERT INTO source_selector(concept_id,system_id,selector) SELECT id+1,'lcc','PR1490..PR1502' FROM _oe_base;
INSERT INTO source_selector(concept_id,system_id,selector) SELECT id+2,'lcc','PR1509..PR1799' FROM _oe_base;
INSERT INTO source_selector(concept_id,system_id,selector) SELECT id+3,'lcc','PR1799.2' FROM _oe_base;

DELETE FROM source_selector WHERE concept_id=4401 AND system_id='lcc';

DROP TABLE _oe_base;
COMMIT;

-- Replace Biology's enumerated general and developmental blocks with ranged leaves.
PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;

CREATE TEMP TABLE _bio_base(id INTEGER NOT NULL);
INSERT INTO _bio_base SELECT COALESCE(MAX(concept_id),0) FROM concept;
INSERT INTO concept(concept_id,preferred_label) SELECT id+1,'General Biology & Research' FROM _bio_base;
INSERT INTO concept(concept_id,preferred_label) SELECT id+2,'Development & Regeneration' FROM _bio_base;
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal) SELECT id+1,5829,0 FROM _bio_base;
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal) SELECT id+2,4789,0 FROM _bio_base;
INSERT INTO source_selector(concept_id,system_id,selector)
SELECT id+1,'lcc','QH301..QH355' FROM _bio_base;
INSERT INTO source_selector(concept_id,system_id,selector)
SELECT id+2,'lcc','QH491..QH492' FROM _bio_base;
INSERT INTO source_selector(concept_id,system_id,selector)
SELECT id+2,'lcc','QH499' FROM _bio_base;

DELETE FROM source_selector WHERE concept_id=4789 AND system_id='lcc';

DROP TABLE _bio_base;
COMMIT;

PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;
CREATE TEMP TABLE _sea_base(id INTEGER NOT NULL);
INSERT INTO _sea_base SELECT COALESCE(MAX(concept_id),0) FROM concept;
INSERT INTO concept(concept_id,preferred_label) SELECT id+1,'General & Regional Southeast Asia' FROM _sea_base;
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal) SELECT id+1,3509,0 FROM _sea_base;
INSERT INTO source_selector(concept_id,system_id,selector)
SELECT id+1,'lcc','DS520..DS526.7' FROM _sea_base;
INSERT INTO source_selector(concept_id,system_id,selector)
SELECT id+1,'lcc','DS526.9' FROM _sea_base;
DELETE FROM source_selector WHERE concept_id=3509 AND system_id='lcc' AND selector IN ('DS520..DS526.7','DS526.9');
DROP TABLE _sea_base;
COMMIT;

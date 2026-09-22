-- A pruned modern-Iraq era is intentionally a broad leaf and can own the
-- complete Republic span. Move enumerated Tunisian city cutters to Tunisia.
PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;

DELETE FROM source_selector WHERE concept_id=11405 AND system_id='lcc';
INSERT INTO source_selector VALUES(11405,'lcc','DS79.65..DS79.769');

CREATE TEMP TABLE _it_base(id INTEGER NOT NULL);
INSERT INTO _it_base SELECT COALESCE(MAX(concept_id),0) FROM concept;
INSERT INTO concept(concept_id,preferred_label)
SELECT id+1,'Cities & Towns' FROM _it_base;
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT id+1,3371,0 FROM _it_base;
INSERT INTO source_selector(concept_id,system_id,selector)
SELECT id+1,'lcc','DT269.A..DT269.Z' FROM _it_base;
DELETE FROM source_selector WHERE concept_id=3349 AND system_id='lcc'
 AND selector GLOB 'DT269.*';
DROP TABLE _it_base;
COMMIT;

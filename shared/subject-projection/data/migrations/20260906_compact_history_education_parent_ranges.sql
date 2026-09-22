-- Finish History of Education range redistribution.
PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;

DELETE FROM source_selector WHERE concept_id=2206 AND system_id='lcc';
INSERT INTO source_selector VALUES(2206,'lcc','LA173..LA186.22');

DELETE FROM source_selector WHERE concept_id=2208 AND system_id='lcc';
INSERT INTO source_selector VALUES(2208,'lcc','LA201..LA398.2');

CREATE TEMP TABLE _he_base(id INTEGER NOT NULL);
INSERT INTO _he_base SELECT COALESCE(MAX(concept_id),0) FROM concept;
INSERT INTO concept(concept_id,preferred_label) SELECT id+1,'Americas—General' FROM _he_base;
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal) SELECT id+1,2207,0 FROM _he_base;
INSERT INTO source_selector(concept_id,system_id,selector) SELECT id+1,'lcc','LA190..LA198.7' FROM _he_base;

-- Preserve only general exact codes on the nonleaf.
DELETE FROM source_selector
WHERE concept_id=2203 AND system_id='lcc'
  AND selector NOT IN ('LA','LA5','LA7','LA9','LA11','LA13','LA14','LA21','LA23','LA25');

DROP TABLE _he_base;
COMMIT;

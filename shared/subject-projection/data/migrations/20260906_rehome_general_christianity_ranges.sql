-- BR1-BR139 is Christianity general/reference, not Christianity in relation
-- to arts and culture. Move it to a proper terminal leaf under Christianity.
-- The remaining BR1610/BR1625 blocks form a broad terminal movement leaf.
PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;

CREATE TEMP TABLE _christ_base(id INTEGER NOT NULL);
INSERT INTO _christ_base SELECT COALESCE(MAX(concept_id),0) FROM concept;

INSERT INTO concept(concept_id,preferred_label)
SELECT id+1,'General Christianity & Reference' FROM _christ_base;
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT id+1,4582,COALESCE((SELECT MAX(ordinal)+1 FROM concept_parent WHERE parent_concept_id=4582),0)
FROM _christ_base;
INSERT INTO source_selector(concept_id,system_id,selector)
SELECT id+1,'lcc','BR1..BR139' FROM _christ_base;

INSERT INTO concept(concept_id,preferred_label)
SELECT id+2,'Tolerance & Transdenominational Movements' FROM _christ_base;
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT id+2,4586,COALESCE((SELECT MAX(ordinal)+1 FROM concept_parent WHERE parent_concept_id=4586),0)
FROM _christ_base;
INSERT INTO source_selector(concept_id,system_id,selector)
SELECT id+2,'lcc','BR1610..BR1610.2' FROM _christ_base;
INSERT INTO source_selector(concept_id,system_id,selector)
SELECT id+2,'lcc','BR1625..BR1665.2' FROM _christ_base;

DELETE FROM source_selector WHERE concept_id=4586 AND system_id='lcc';

DROP TABLE _christ_base;
COMMIT;

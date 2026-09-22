-- Compact Oceania's general schedule and the deliberately pruned smaller-
-- island schedule. Specific broad-region children retain their exact headings.
PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;

CREATE TEMP TABLE _oceania_base(id INTEGER NOT NULL);
INSERT INTO _oceania_base SELECT COALESCE(MAX(concept_id),0) FROM concept;

INSERT INTO concept(concept_id,preferred_label)
SELECT id+1,'General & Reference' FROM _oceania_base;
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT id+1,3676,COALESCE((SELECT MAX(ordinal)+1 FROM concept_parent WHERE parent_concept_id=3676),0)
FROM _oceania_base;
INSERT INTO source_selector(concept_id,system_id,selector)
SELECT id+1,'lcc','DU1..DU18.5' FROM _oceania_base;
INSERT INTO source_selector(concept_id,system_id,selector)
SELECT id+1,'lcc','DU28.1..DU28.34' FROM _oceania_base;

INSERT INTO concept(concept_id,preferred_label)
SELECT id+2,'Smaller Pacific Island Groups' FROM _oceania_base;
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT id+2,3676,COALESCE((SELECT MAX(ordinal)+1 FROM concept_parent WHERE parent_concept_id=3676),0)
FROM _oceania_base;
INSERT INTO source_selector(concept_id,system_id,selector)
SELECT id+2,'lcc','DU520..DU950.Z' FROM _oceania_base;

DELETE FROM source_selector WHERE concept_id=3676 AND system_id='lcc';
INSERT INTO source_selector VALUES(3676,'lcc','DU');

DROP TABLE _oceania_base;
COMMIT;

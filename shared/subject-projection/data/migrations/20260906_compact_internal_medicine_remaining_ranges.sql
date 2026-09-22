-- Compact general Internal Medicine and remove ancestor copies covered by children.
PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;

CREATE TEMP TABLE _im_base(id INTEGER NOT NULL);
INSERT INTO _im_base SELECT COALESCE(MAX(concept_id),0) FROM concept;
INSERT INTO concept(concept_id,preferred_label)
SELECT id+1,'General Internal Medicine' FROM _im_base;
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT id+1,2707,0 FROM _im_base;
INSERT INTO source_selector(concept_id,system_id,selector)
SELECT id+1,'lcc','RC31..RC69' FROM _im_base;

-- These belong to the existing neuroscience child rather than its ancestor.
INSERT OR IGNORE INTO source_selector VALUES
 (2713,'lcc','RC332'),(2713,'lcc','RC333'),(2713,'lcc','RC337.8'),
 (2713,'lcc','RC340'),(2713,'lcc','RC341');

-- Keep only uncovered exact umbrella codes on the Internal Medicine parent.
DELETE FROM source_selector
WHERE concept_id=2707 AND system_id='lcc'
  AND selector NOT IN ('RC','RC90','RC103.2','RC106','RC107','RC108','RC251');

DROP TABLE _im_base;
COMMIT;

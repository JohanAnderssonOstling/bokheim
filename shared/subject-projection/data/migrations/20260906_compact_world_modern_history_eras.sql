-- Replace enumerated modern-history events, wars, and individual numbers with
-- broad chronological leaves.
PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;

DELETE FROM source_selector WHERE system_id='lcc' AND concept_id IN
 (3701,3666,3667,3668,3669,3670,3671);
INSERT INTO source_selector VALUES
 (3667,'lcc','D219..D241.Z'),
 (3668,'lcc','D242..D283.5'),
 (3669,'lcc','D284..D297'),
 (3670,'lcc','D299..D400.Z'),
 (3671,'lcc','D410..D475.2');
UPDATE concept SET preferred_label='Early Modern Era, 1453–1648' WHERE concept_id=3667;
UPDATE concept SET preferred_label='Seventeenth Century, 1601–1715' WHERE concept_id=3668;
UPDATE concept SET preferred_label='Eighteenth Century, 1715–1789' WHERE concept_id=3669;
UPDATE concept SET preferred_label='Long Nineteenth Century, 1789–1914/1920' WHERE concept_id=3670;

CREATE TEMP TABLE _world_modern_base(id INTEGER NOT NULL);
INSERT INTO _world_modern_base SELECT COALESCE(MAX(concept_id),0) FROM concept;
INSERT INTO concept(concept_id,preferred_label)
SELECT id+1,'Modern History—General & Reference' FROM _world_modern_base;
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT id+1,3666,COALESCE((SELECT MAX(ordinal)+1 FROM concept_parent WHERE parent_concept_id=3666),0)
FROM _world_modern_base;
INSERT INTO source_selector
SELECT id+1,'lcc','D203.2..D218.Z' FROM _world_modern_base;
DROP TABLE _world_modern_base;

COMMIT;

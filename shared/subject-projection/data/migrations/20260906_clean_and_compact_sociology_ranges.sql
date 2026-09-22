-- Remove selectors imported into Sociology from unrelated H, HJ and HX
-- schedules, then compact the coherent modern-HM reference blocks into leaves.
PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;

-- H1-H99 is already owned by Social Sciences. HX belongs to the existing
-- Socialism & Communism branch.
DELETE FROM source_selector
WHERE concept_id=5488 AND system_id='lcc'
  AND (selector GLOB 'H[0-9]*' OR selector GLOB 'H.*');
DELETE FROM source_selector
WHERE concept_id=5488 AND system_id='lcc' AND selector='HX';
INSERT OR IGNORE INTO source_selector VALUES(11761,'lcc','HX');

-- This node had inherited the entire HJ public-finance schedule and an HX
-- regional umbrella. Its actual HM social-change coverage is on its children.
DELETE FROM source_selector WHERE concept_id=5665 AND system_id='lcc';
DELETE FROM concept_parent WHERE concept_id=5209 AND parent_concept_id=5665;

-- Selectors already assigned more specifically to existing children.
DELETE FROM source_selector
WHERE concept_id=5488 AND system_id='lcc'
  AND selector IN ('HM479.2..HM499','HM548','HM554');

CREATE TEMP TABLE _soc_base(id INTEGER NOT NULL);
INSERT INTO _soc_base SELECT COALESCE(MAX(concept_id),0) FROM concept;

INSERT INTO concept(concept_id,preferred_label)
SELECT id+1,'Reference, Organizations & General Works' FROM _soc_base;
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT id+1,5488,COALESCE((SELECT MAX(ordinal)+1 FROM concept_parent WHERE parent_concept_id=5488),0)
FROM _soc_base;
INSERT INTO source_selector(concept_id,system_id,selector)
SELECT id+1,'lcc','HM401..HM429' FROM _soc_base;
INSERT INTO source_selector(concept_id,system_id,selector)
SELECT id+1,'lcc','HM581..HM608' FROM _soc_base;

INSERT INTO concept(concept_id,preferred_label)
SELECT id+2,'Relations with Other Fields & Communication' FROM _soc_base;
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT id+2,5488,COALESCE((SELECT MAX(ordinal)+1 FROM concept_parent WHERE parent_concept_id=5488),0)
FROM _soc_base;
INSERT INTO source_selector(concept_id,system_id,selector)
SELECT id+2,'lcc','HM545..HM547.Z' FROM _soc_base;
INSERT INTO source_selector(concept_id,system_id,selector)
SELECT id+2,'lcc','HM549..HM553.Z' FROM _soc_base;
INSERT INTO source_selector(concept_id,system_id,selector)
SELECT id+2,'lcc','HM555..HM569' FROM _soc_base;

DELETE FROM source_selector
WHERE concept_id=5488 AND system_id='lcc' AND selector IN (
 'HM401','HM403','HM405','HM407','HM409','HM411','HM413','HM415',
 'HM417','HM417.A','HM417.Z','HM419','HM421','HM425','HM427','HM429',
 'HM545','HM551','HM561','HM563','HM567','HM569',
 'HM581','HM582','HM582.A','HM582.Z','HM585','HM586','HM588','HM590',
 'HM592','HM594','HM596','HM598','HM600','HM602','HM604','HM606',
 'HM606.A','HM606.Z','HM608'
);

DROP TABLE _soc_base;
COMMIT;

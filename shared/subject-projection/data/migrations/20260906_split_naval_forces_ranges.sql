-- Move the naval V-class schedules off the Military Studies umbrella into a
-- deliberately broad Naval Forces leaf. Collapse the UH services leaf.
PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;

CREATE TEMP TABLE _naval_base(id INTEGER NOT NULL);
INSERT INTO _naval_base SELECT COALESCE(MAX(concept_id),0) FROM concept;
INSERT INTO concept(concept_id,preferred_label)
SELECT id+1,'Naval Forces' FROM _naval_base;
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT id+1,5254,0 FROM _naval_base;
INSERT INTO source_selector(concept_id,system_id,selector)
SELECT id+1,'lcc',selector FROM _naval_base CROSS JOIN (
 SELECT 'V1..V995' selector UNION ALL
 SELECT 'VA10..VA750' UNION ALL SELECT 'VB15..VB955' UNION ALL
 SELECT 'VC10..VC580' UNION ALL SELECT 'VD7..VD430' UNION ALL
 SELECT 'VF1..VF580' UNION ALL SELECT 'VG20..VG2029' UNION ALL
 SELECT 'VK1..VK1661' UNION ALL SELECT 'VM1..VM989'
);

DELETE FROM source_selector WHERE concept_id=5254 AND system_id='lcc'
 AND selector GLOB 'V*';

DELETE FROM source_selector WHERE concept_id=5261 AND system_id='lcc';
INSERT INTO source_selector VALUES(5261,'lcc','UH20..UH910');

DROP TABLE _naval_base;
COMMIT;

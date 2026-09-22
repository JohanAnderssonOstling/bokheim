-- Compact Spain's general and chronological schedules, then carve the local
-- history fallback around every existing named region/city child.
PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;

DELETE FROM source_selector WHERE concept_id=3601 AND system_id='lcc';
INSERT INTO source_selector VALUES(3601,'lcc','DP160.9..DP272.4');
DELETE FROM source_selector WHERE concept_id=10129 AND system_id='lcc';
INSERT INTO source_selector VALUES(10129,'lcc','DP97.3..DP160.8');

CREATE TEMP TABLE _spain_base(id INTEGER NOT NULL);
INSERT INTO _spain_base SELECT COALESCE(MAX(concept_id),0) FROM concept;

INSERT INTO concept(concept_id,preferred_label)
SELECT id+1,'General & Reference' FROM _spain_base;
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT id+1,3613,COALESCE((SELECT MAX(ordinal)+1 FROM concept_parent WHERE parent_concept_id=3613),0)
FROM _spain_base;
INSERT INTO source_selector(concept_id,system_id,selector)
SELECT id+1,'lcc','DP1..DP86' FROM _spain_base;

INSERT INTO concept(concept_id,preferred_label)
SELECT id+2,'Other Regions & Cities' FROM _spain_base;
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT id+2,3613,COALESCE((SELECT MAX(ordinal)+1 FROM concept_parent WHERE parent_concept_id=3613),0)
FROM _spain_base;
INSERT INTO source_selector(concept_id,system_id,selector)
SELECT id+2,'lcc',selector FROM _spain_base CROSS JOIN (
 SELECT 'DP280..DP302.A40' selector UNION ALL
 SELECT 'DP302.A56..DP302.B15.Z' UNION ALL
 SELECT 'DP302.B31..DP302.B40.Z' UNION ALL
 SELECT 'DP302.B56..DP302.C35.Z' UNION ALL
 SELECT 'DP302.C52..DP302.C56.Z' UNION ALL
 SELECT 'DP302.C70..DP302.G10.Z' UNION ALL
 SELECT 'DP302.G21..DP302.V10.Z' UNION ALL
 SELECT 'DP302.V26..DP349.Z' UNION ALL
 SELECT 'DP375..DP402.B19.Z' UNION ALL
 SELECT 'DP402.B31..DP402.Z'
);

DELETE FROM source_selector WHERE concept_id=3613 AND system_id='lcc';
INSERT INTO source_selector VALUES(3613,'lcc','DP492');

DROP TABLE _spain_base;
COMMIT;

-- Distribute the English-language PE schedule to non-overlapping terminal
-- topic leaves, including the three previously selectorless curated children.
PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;

DELETE FROM source_selector WHERE concept_id=12188 AND system_id='lcc';
INSERT INTO source_selector VALUES(12188,'lcc','PE1401.2..PE1497');
DELETE FROM source_selector WHERE concept_id=12190 AND system_id='lcc';
INSERT INTO source_selector VALUES(12190,'lcc','PE1108..PE1127.Z');
DELETE FROM source_selector WHERE concept_id=12191 AND system_id='lcc';
INSERT INTO source_selector VALUES(12191,'lcc','PE1711..PE3602');

DELETE FROM source_selector WHERE concept_id=12196 AND system_id='lcc';
INSERT INTO source_selector VALUES(12196,'lcc','PE101..PE458.2');
INSERT INTO source_selector VALUES(12196,'lcc','PE501..PE693.2');
INSERT INTO source_selector VALUES(12196,'lcc','PE801..PE896');
INSERT INTO source_selector VALUES(12196,'lcc','PE1075..PE1088.2');

CREATE TEMP TABLE _englang_base(id INTEGER NOT NULL);
INSERT INTO _englang_base SELECT COALESCE(MAX(concept_id),0) FROM concept;
INSERT INTO concept(concept_id,preferred_label)
SELECT id+1,'General Philology & Language Reference' FROM _englang_base;
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT id+1,3709,COALESCE((SELECT MAX(ordinal)+1 FROM concept_parent WHERE parent_concept_id=3709),0)
FROM _englang_base;
INSERT INTO source_selector(concept_id,system_id,selector)
SELECT id+1,'lcc',selector FROM _englang_base CROSS JOIN (
 SELECT 'PE1..PE71' selector UNION ALL
 SELECT 'PE1001..PE1064.Z' UNION ALL
 SELECT 'PE1070..PE1074.Z' UNION ALL
 SELECT 'PE1089..PE1096.Z'
);

DELETE FROM source_selector WHERE concept_id=3709 AND system_id='lcc';
INSERT INTO source_selector VALUES(3709,'lcc','PE');

DROP TABLE _englang_base;
COMMIT;

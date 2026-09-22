-- Further official whole-leaf and Cutter-range consolidations.
PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;

DELETE FROM source_selector WHERE concept_id=109 AND system_id='lcc';
INSERT INTO source_selector VALUES(109,'lcc','PN4550..PN4590');

DELETE FROM source_selector WHERE concept_id=12757 AND system_id='lcc';
INSERT INTO source_selector VALUES(12757,'lcc','DP302.V11..DP302.V25');

DELETE FROM source_selector WHERE concept_id=3900 AND system_id='lcc';
INSERT INTO source_selector VALUES(3900,'lcc','PT5600..PT5739');

DELETE FROM source_selector WHERE concept_id=4380 AND system_id='lcc';
INSERT INTO source_selector VALUES
  (4380,'lcc','PN695..PN779'),
  (4380,'lcc','PN780..PN789');

DELETE FROM source_selector WHERE concept_id=8841 AND system_id='lcc';
INSERT INTO source_selector VALUES
  (8841,'lcc','RD85.5'),
  (8841,'lcc','RD86.A..RD86.Z');

DELETE FROM source_selector WHERE concept_id=8238 AND system_id='lcc';
INSERT INTO source_selector VALUES
  (8238,'lcc','BJ1534'),
  (8238,'lcc','BJ1535.A..BJ1535.Z');

-- The enclosing Saxony range already exists; discard its redundant members.
DELETE FROM source_selector
WHERE concept_id=11450 AND system_id='lcc'
  AND selector LIKE 'DD801.S%' AND instr(selector,'..')=0;

COMMIT;

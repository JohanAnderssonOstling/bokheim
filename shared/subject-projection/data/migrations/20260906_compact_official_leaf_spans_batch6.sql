-- Bibliography, psychology Cutter blocks, regional history, and property crime.
PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;

DELETE FROM source_selector WHERE concept_id=8370 AND system_id='lcc';
INSERT INTO source_selector VALUES
  (8370,'lcc','Z2001..Z2029'),
  (8370,'lcc','Z2031..Z2049'),
  (8370,'lcc','Z2051..Z2069'),
  (8370,'lcc','Z2071..Z2089');

DELETE FROM source_selector WHERE concept_id=10774 AND system_id='lcc';
INSERT INTO source_selector VALUES
  (10774,'lcc','BF724'),
  (10774,'lcc','BF724.3.A..BF724.3.Z');

DELETE FROM source_selector WHERE concept_id=10964 AND system_id='lcc';
INSERT INTO source_selector VALUES
  (10964,'lcc','BF719'),
  (10964,'lcc','BF720.A..BF720.Z');

DELETE FROM source_selector WHERE concept_id=12750 AND system_id='lcc';
INSERT INTO source_selector VALUES(12750,'lcc','DP302.B41..DP302.B55');

DELETE FROM source_selector WHERE concept_id=12729 AND system_id='lcc';
INSERT INTO source_selector VALUES(12729,'lcc','HV6635..HV6700');

COMMIT;

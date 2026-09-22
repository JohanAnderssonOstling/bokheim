-- Compact fully pruned leaves whose enumerations are covered by explicit LCC
-- schedule spans.
PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;

DELETE FROM source_selector WHERE concept_id=12727 AND system_id='lcc';
INSERT INTO source_selector VALUES(12727,'lcc','HV6250..HV6250.4.Z');

DELETE FROM source_selector WHERE concept_id=9826 AND system_id='lcc';
INSERT INTO source_selector VALUES(9826,'lcc','Z253..Z253.532.Z');

DELETE FROM source_selector WHERE concept_id=7343 AND system_id='lcc';
INSERT INTO source_selector VALUES(7343,'lcc','LB1044.88..LB1044.9.Z');

DELETE FROM source_selector WHERE concept_id=7888 AND system_id='lcc';
INSERT INTO source_selector VALUES(7888,'lcc','P305..P305.19.Z');

DELETE FROM source_selector WHERE concept_id=13067 AND system_id='lcc';
INSERT INTO source_selector VALUES(13067,'lcc','TK7868.A..TK7868.Z');

DELETE FROM source_selector WHERE concept_id=10534 AND system_id='lcc';
INSERT INTO source_selector VALUES
 (10534,'lcc','DT644.8'),(10534,'lcc','DT648'),(10534,'lcc','DT649'),
 (10534,'lcc','DT665.A..DT665.Z');

DELETE FROM source_selector WHERE concept_id=5454 AND system_id='lcc';
INSERT INTO source_selector VALUES
 (5454,'lcc','BF207'),(5454,'lcc','BF209.A..BF209.Z');

DELETE FROM source_selector WHERE concept_id=6573 AND system_id='lcc';
INSERT INTO source_selector VALUES(6573,'lcc','NC1765..NC1766.5.Z');

DELETE FROM source_selector WHERE concept_id=8171 AND system_id='lcc';
INSERT INTO source_selector VALUES(8171,'lcc','RD755..RD757.Z');

COMMIT;

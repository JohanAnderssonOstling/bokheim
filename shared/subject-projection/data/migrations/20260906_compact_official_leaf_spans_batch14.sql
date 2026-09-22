-- Compact smaller verified schedule spans. Retain isolated codes where LCC
-- does not supply a continuous span.
PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;

DELETE FROM source_selector WHERE concept_id=10594 AND system_id='lcc';
INSERT INTO source_selector VALUES
 (10594,'lcc','D805.A..D805.Z'),
 (10594,'lcc','D805.5..D805.6.Z');

DELETE FROM source_selector WHERE concept_id=4481 AND system_id='lcc';
INSERT INTO source_selector VALUES(4481,'lcc','PH91.A1..PH98.5');

DELETE FROM source_selector WHERE concept_id=4938 AND system_id='lcc';
INSERT INTO source_selector VALUES(4938,'lcc','QA269..QA272');

DELETE FROM source_selector WHERE concept_id=6146 AND system_id='lcc';
INSERT INTO source_selector VALUES
 (6146,'lcc','Z666.5..Z666.7'),
 (6146,'lcc','Z666.73.A..Z666.73.Z');

DELETE FROM source_selector WHERE concept_id=6455 AND system_id='lcc';
INSERT INTO source_selector VALUES
 (6455,'lcc','TC1665..TC1670'),
 (6455,'lcc','TC1680'),
 (6455,'lcc','TC1700..TC1703');

DELETE FROM source_selector WHERE concept_id=8887 AND system_id='lcc';
INSERT INTO source_selector VALUES
 (8887,'lcc','ND3332..ND3343.5'),
 (8887,'lcc','ND3343.7'),(8887,'lcc','ND3343.8'),
 (8887,'lcc','ND3343.9'),(8887,'lcc','ND3344');

DELETE FROM source_selector WHERE concept_id=10102 AND system_id='lcc';
INSERT INTO source_selector VALUES(10102,'lcc','DF235..DF238.9.Z');

DELETE FROM source_selector WHERE concept_id=11531 AND system_id='lcc';
INSERT INTO source_selector VALUES(11531,'lcc','DJ215..DJ295');

DELETE FROM source_selector WHERE concept_id=11536 AND system_id='lcc';
INSERT INTO source_selector VALUES(11536,'lcc','DL503..DL526');

COMMIT;

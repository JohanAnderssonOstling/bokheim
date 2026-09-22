-- Compact further explicit official spans and remove exact selectors already
-- subsumed by an existing leaf range.
PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;

DELETE FROM source_selector WHERE concept_id=12755 AND system_id='lcc';
INSERT INTO source_selector VALUES(12755,'lcc','DP402.B2..DP402.B3');

DELETE FROM source_selector WHERE concept_id=12752 AND system_id='lcc';
INSERT INTO source_selector VALUES(12752,'lcc','DP302.G11..DP302.G2');

DELETE FROM source_selector WHERE concept_id=11447 AND system_id='lcc';
INSERT INTO source_selector VALUES(11447,'lcc','DD801.B31..DD801.B43');

DELETE FROM source_selector WHERE concept_id=11451 AND system_id='lcc';
INSERT INTO source_selector VALUES(11451,'lcc','DD801.W6..DD801.W82');

DELETE FROM source_selector WHERE concept_id=3599 AND system_id='lcc';
INSERT INTO source_selector VALUES(3599,'lcc','DP212..DP220');
-- The same official era span had been retained as an ancestor fallback.
DELETE FROM source_selector
WHERE concept_id=3613 AND system_id='lcc' AND selector='DP212..DP220';

DELETE FROM source_selector WHERE concept_id=6582 AND system_id='lcc';
INSERT INTO source_selector VALUES(6582,'lcc','TN410..TN438.Z');

COMMIT;

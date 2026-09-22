-- Remove Management parent selectors already covered by existing ranged
-- leaves. Preserve unassigned management codes on the parent.
PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;

DELETE FROM source_selector WHERE concept_id=11592 AND system_id='lcc';
INSERT INTO source_selector VALUES
 (11592,'lcc','HD49..HD49.6'),
 (11592,'lcc','HD61..HD61.5');

DELETE FROM source_selector WHERE concept_id=11594 AND system_id='lcc';
INSERT INTO source_selector VALUES(11594,'lcc','HD60..HD60.5');

DELETE FROM source_selector
WHERE concept_id=5236 AND system_id='lcc' AND (
 selector='HD28-70'
 OR (selector GLOB 'HD[0-9]*' AND (
   CAST(substr(selector,3) AS REAL) BETWEEN 38 AND 38.5
   OR CAST(substr(selector,3) AS REAL) BETWEEN 45 AND 45.2
   OR CAST(substr(selector,3) AS REAL) BETWEEN 49 AND 49.6
   OR CAST(substr(selector,3) AS REAL) BETWEEN 56 AND 57.5
   OR CAST(substr(selector,3) AS REAL) BETWEEN 58.7 AND 58.95
   OR CAST(substr(selector,3) AS REAL) BETWEEN 60 AND 60.5
   OR CAST(substr(selector,3) AS REAL) BETWEEN 61 AND 61.5
   OR CAST(substr(selector,3) AS REAL) BETWEEN 62.2 AND 62.8
   OR CAST(substr(selector,3) AS REAL) BETWEEN 66 AND 66.5
 ))
);

COMMIT;

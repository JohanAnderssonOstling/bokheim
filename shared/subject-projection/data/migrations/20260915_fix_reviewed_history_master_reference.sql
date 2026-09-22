-- Apply only to master-taxonomy-v2.sqlite3, not the curated database.
PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;
-- Ancient Egypt was incorrectly nested among Lesotho's periods.
UPDATE concept_parent SET parent_concept_id=3635
WHERE concept_id=33844 AND parent_concept_id=31803;
-- DS556–DS559.93 is Vietnam, not the French Indochina umbrella.
UPDATE source_selector SET concept_id=3513
WHERE concept_id=3495 AND system_id='lcc' AND selector='DS556..DS559.93';
-- DT633 is the early Liberian period; DT634 onward covers later periods.
UPDATE source_selector SET selector='DT633..DT633'
WHERE concept_id=33433 AND system_id='lcc' AND selector='DT633..DT636.3';
-- The imported Tunisian ethnography node also absorbed other countries.
UPDATE source_selector SET concept_id=3349
WHERE concept_id=26300 AND system_id='lcc' AND selector='DT193..DT193.95';
DELETE FROM source_selector WHERE concept_id=26300 AND system_id='lcc'
AND selector='DT223..DT223.2' AND EXISTS
 (SELECT 1 FROM source_selector s WHERE s.concept_id=3348 AND s.system_id='lcc'
  AND s.selector='DT223..DT223.2');
COMMIT;

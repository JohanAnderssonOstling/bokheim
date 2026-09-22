-- A broad browse concept owns broad fallback coverage only. More precise LCC
-- ranges remain on states, periods, cities, and master classification detail.
-- This migration is intentionally idempotent.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

DELETE FROM source_selector
WHERE concept_id = 'unified:1fd4042d-5f0d-5835-ac17-9bc708487a6a'
  AND system_id = 'lcc'
  AND selector NOT IN ('E151..E889@1', 'F1..F975@1');

COMMIT;

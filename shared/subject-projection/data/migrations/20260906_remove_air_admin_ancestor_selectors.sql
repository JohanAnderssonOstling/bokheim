-- Air Forces and Administration/Logistics/Engineering are fully represented
-- by their existing ranged leaves. Remove overlapping nonleaf fallback copies.
PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;
DELETE FROM source_selector
WHERE concept_id IN (5849,5850) AND system_id='lcc';
COMMIT;

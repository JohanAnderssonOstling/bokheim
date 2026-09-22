-- The ten Demography leaves form a complete HB848..HB3697 partition. Remove
-- the nonleaf fallback enumeration and the obsolete all-demography umbrella.
PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;
DELETE FROM source_selector WHERE concept_id=5096 AND system_id='lcc';
DELETE FROM source_selector WHERE concept_id=12879 AND system_id='lcc'
 AND selector='HB848-3697';
COMMIT;

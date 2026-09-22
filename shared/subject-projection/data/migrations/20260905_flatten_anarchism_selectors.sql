PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;

DELETE FROM source_selector
WHERE concept_id='bokheim:politics:ideology:anarchism'
  AND system_id='lcc'
  AND selector<>'HX821..HX970.9@1';

UPDATE source_selector SET ordinal=0
WHERE concept_id='bokheim:politics:ideology:anarchism'
  AND system_id='lcc'
  AND selector='HX821..HX970.9@1';

COMMIT;

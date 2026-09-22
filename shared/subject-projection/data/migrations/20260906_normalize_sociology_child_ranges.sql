-- Make the HM sociology branches mutually exclusive and remove restored parent
-- selectors covered by those child ranges.
PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;

-- Normalize legacy hyphen spellings and redundant equivalent ranges.
DELETE FROM source_selector WHERE concept_id=12256 AND system_id='lcc'
 AND selector IN ('HM621-656','HM621-HM656');

UPDATE concept SET preferred_label='Groups & Group Dynamics' WHERE concept_id=12257;
DELETE FROM source_selector WHERE concept_id=12257 AND system_id='lcc'
 AND selector IN ('HM711..HM755');
INSERT OR IGNORE INTO source_selector VALUES
 (12257,'lcc','HM711..HM740.9'),
 (12257,'lcc','HM743.1..HM752.9');

-- Reserve HM866..HM886 for Collective Behavior & Social Movements.
DELETE FROM source_selector WHERE concept_id=12255 AND system_id='lcc'
 AND selector IN ('HM831..HM901','HM846-851');
INSERT OR IGNORE INTO source_selector VALUES
 (12255,'lcc','HM831..HM865.9'),
 (12255,'lcc','HM887..HM901');

-- Remove every direct Sociology selector whose numeric HM portion is covered
-- by an existing ranged descendant.
DELETE FROM source_selector
WHERE concept_id=5488 AND system_id='lcc' AND selector GLOB 'HM[0-9]*'
 AND (
   (CAST(substr(selector,3) AS REAL) BETWEEN 435 AND 477.999)
   OR (CAST(substr(selector,3) AS REAL) BETWEEN 621 AND 696.999)
   OR (CAST(substr(selector,3) AS REAL) BETWEEN 701 AND 708.999)
   OR (CAST(substr(selector,3) AS REAL) BETWEEN 711 AND 821.999)
   OR (CAST(substr(selector,3) AS REAL) BETWEEN 826 AND 901.999)
 );

-- Remove obsolete umbrella spellings even when their parsed start lies just
-- outside the numeric predicates above.
DELETE FROM source_selector WHERE concept_id=5488 AND system_id='lcc'
 AND selector IN (
  'HM401-1281','HM401..HM1281','HM479.2-554','HM479.2..HM554',
  'HM711-806','HM711..HM806','HM716-753.2','HM716..HM753.2'
 );

COMMIT;

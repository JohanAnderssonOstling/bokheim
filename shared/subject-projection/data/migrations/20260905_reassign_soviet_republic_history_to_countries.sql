-- Keep union-wide history on the Soviet Union concept, but assign national DK
-- schedules directly to the existing country concepts (including non-leaves).

PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;

-- These generated catch-alls span every country and prevent exclusive ownership.
DELETE FROM source_selector
WHERE concept_id='bokheim:history:soviet-union-former-republics'
  AND selector IN ('DK1..DK949.5@1','DK501..DK949.5@2');

-- Move already-curated detailed national selectors out of the Soviet leaf.
DROP TABLE IF EXISTS temp.soviet_country_move;
CREATE TEMP TABLE soviet_country_move AS
SELECT s.system_id,s.selector,
 CASE
  WHEN CAST(substr(s.selector,3) AS REAL) BETWEEN 401 AND 444.99 THEN 'unified:b39c446a-3154-521c-8bf4-fa9c69c84ea5'
  WHEN CAST(substr(s.selector,3) AS REAL) BETWEEN 445 AND 465.99 THEN 'unified:550fd12f-93fc-55dc-8a89-eefb55a0ff09'
  WHEN CAST(substr(s.selector,3) AS REAL) BETWEEN 508 AND 508.99 THEN 'unified:cad75fff-9e45-5455-bab5-97baa628746d'
  WHEN CAST(substr(s.selector,3) AS REAL) BETWEEN 510 AND 651.99 THEN 'unified:55b70b6c-2304-5d17-80d5-c4c687b80d10'
  WHEN CAST(substr(s.selector,3) AS REAL) BETWEEN 751 AND 781.99 THEN 'unified:55b70b6c-2304-5d17-80d5-c4c687b80d10'
 END target_id
FROM source_selector s
WHERE s.concept_id='bokheim:history:soviet-union-former-republics'
  AND s.system_id='lcc';
DELETE FROM soviet_country_move WHERE target_id IS NULL;

INSERT OR IGNORE INTO source_selector(concept_id,system_id,selector,ordinal)
SELECT m.target_id,m.system_id,m.selector,
 COALESCE((SELECT max(x.ordinal)+1 FROM source_selector x
           WHERE x.concept_id=m.target_id AND x.system_id=m.system_id),0)
 + row_number() OVER (PARTITION BY m.target_id,m.system_id ORDER BY m.selector)-1
FROM soviet_country_move m;
DELETE FROM source_selector
WHERE concept_id='bokheim:history:soviet-union-former-republics'
  AND EXISTS (SELECT 1 FROM soviet_country_move m
              WHERE m.system_id=source_selector.system_id
                AND m.selector=source_selector.selector);

-- Broad national schedules belong directly to their countries. Their existing
-- period children remain more-specific matches.
INSERT OR IGNORE INTO source_selector(concept_id,system_id,selector,ordinal) VALUES
 ('bokheim:history-lcc-full:1f169c2e-a1e7-51ef-8de2-ecbceb7c6ce6','lcc','DK503..DK503.95@2',10000),
 ('bokheim:history-lcc-full:26912220-a2fa-548e-a7cb-521cd99cd7ea','lcc','DK504..DK504.95@2',10000),
 ('bokheim:history-lcc-full:81349409-31c6-5d92-9e9a-a71164000e3a','lcc','DK505..DK505.95@2',10000),
 ('unified:20a61563-2ae2-55d0-ac1a-11e7a0f7c4c4','lcc','DK507..DK507.95@2',10000),
 ('unified:cad75fff-9e45-5455-bab5-97baa628746d','lcc','DK508..DK508.95@2',10000),
 ('unified:bab49688-fa63-5c8d-89d7-2a2dea66f052','lcc','DK509.1..DK509.95@2',10000),
 ('unified:55b70b6c-2304-5d17-80d5-c4c687b80d10','lcc','DK510..DK651@2',10000),
 ('unified:1b6eac11-c24a-5110-93a1-5a55ba8d4369','lcc','DK670..DK679.5@2',10000),
 ('bokheim:history-lcc-full:e3312677-a702-53c8-8d97-3da56ffc2389','lcc','DK680..DK689.5@2',10000),
 ('unified:18488bf6-5580-5533-bb8e-8bbaaab69cdf','lcc','DK690..DK699.5@2',10000),
 ('unified:55b70b6c-2304-5d17-80d5-c4c687b80d10','lcc','DK751..DK781@2',10001),
 ('unified:c779ab27-d009-55d2-9ed3-5e63e3773683','lcc','DK901..DK909.5@2',10000),
 ('unified:2c6b0df0-0f86-58ea-a360-c7970f56ed8b','lcc','DK911..DK919.5@2',10000),
 ('unified:adac33a9-57da-55f4-a4e4-39458722dc4a','lcc','DK921..DK929.5@2',10000),
 ('unified:da0a14bc-4516-5ce9-8a94-8e1afae7bfe4','lcc','DK931..DK939.5@2',10000),
 ('unified:fd48f946-9e94-5be5-ad45-026e852e21ab','lcc','DK941..DK949.5@2',10000);

COMMIT;

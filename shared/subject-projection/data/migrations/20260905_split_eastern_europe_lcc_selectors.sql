-- Move DK detail away from the Eastern Europe umbrella.  DK is the schedule
-- for Russia, the Soviet Union, and the former Soviet republics; only DJK
-- genuinely describes Eastern Europe as a region.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

INSERT OR IGNORE INTO concept(concept_id,preferred_label) VALUES
 ('bokheim:history:soviet-union-former-republics','Soviet Union & Former Soviet Republics'),
 ('bokheim:history:central-asia','Central Asia');
INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal,display_order) VALUES
 ('bokheim:history:soviet-union-former-republics','unified:49ac1f3c-ff3b-5d4a-b864-83cd3bf050ec',0,105),
 ('bokheim:history:central-asia','bokheim:subject:history:asia',0,125);

UPDATE concept
SET preferred_label='Russia'
WHERE concept_id='unified:55b70b6c-2304-5d17-80d5-c4c687b80d10';

CREATE TEMP TABLE dk_assignment(
 selector TEXT PRIMARY KEY,
 target_id TEXT NOT NULL
) WITHOUT ROWID;

WITH parsed AS (
 SELECT selector,
        CAST(substr(selector,3) AS REAL) AS start_no,
        CASE WHEN instr(selector,'..DK') > 0
             THEN CAST(substr(selector,instr(selector,'..DK')+4) AS REAL)
             ELSE CAST(substr(selector,3) AS REAL) END AS end_no
 FROM source_selector
 WHERE concept_id='unified:49ac1f3c-ff3b-5d4a-b864-83cd3bf050ec'
   AND system_id='lcc' AND selector GLOB 'DK[0-9]*'
)
INSERT INTO dk_assignment(selector,target_id)
SELECT selector, CASE
 -- Existing European country entries.
 WHEN start_no>=502.3 AND end_no<=502.75 THEN 'unified:5af5f58e-058a-5705-bd64-f8e96b4ce270'
 WHEN start_no>=503   AND end_no<=503.95 THEN 'bokheim:history-lcc-full:1f169c2e-a1e7-51ef-8de2-ecbceb7c6ce6'
 WHEN start_no>=504   AND end_no<=504.95 THEN 'bokheim:history-lcc-full:26912220-a2fa-548e-a7cb-521cd99cd7ea'
 WHEN start_no>=505   AND end_no<=505.95 THEN 'bokheim:history-lcc-full:81349409-31c6-5d92-9e9a-a71164000e3a'
 WHEN start_no>=507   AND end_no<=507.95 THEN 'unified:20a61563-2ae2-55d0-ac1a-11e7a0f7c4c4'
 WHEN start_no>=509.1 AND end_no<=509.95 THEN 'unified:bab49688-fa63-5c8d-89d7-2a2dea66f052'
 -- Caucasus country entries live under Asia in the curated geography.
 WHEN start_no>=670   AND end_no<=679.5 THEN 'unified:1b6eac11-c24a-5110-93a1-5a55ba8d4369'
 WHEN start_no>=680   AND end_no<=689.5 THEN 'bokheim:history-lcc-full:e3312677-a702-53c8-8d97-3da56ffc2389'
 WHEN start_no>=690   AND end_no<=699.5 THEN 'unified:18488bf6-5580-5533-bb8e-8bbaaab69cdf'
 -- Central Asian republics likewise live under Asia.
 WHEN start_no>=901   AND end_no<=909.5 THEN 'unified:c779ab27-d009-55d2-9ed3-5e63e3773683'
 WHEN start_no>=911   AND end_no<=919.5 THEN 'unified:2c6b0df0-0f86-58ea-a360-c7970f56ed8b'
 WHEN start_no>=921   AND end_no<=929.5 THEN 'unified:adac33a9-57da-55f4-a4e4-39458722dc4a'
 WHEN start_no>=931   AND end_no<=939.5 THEN 'unified:da0a14bc-4516-5ce9-8a94-8e1afae7bfe4'
 WHEN start_no>=941   AND end_no<=949.5 THEN 'unified:fd48f946-9e94-5be5-ad45-026e852e21ab'
 -- Regional Central Asian material that is not limited to one republic.
 WHEN start_no>=845   AND end_no<901 THEN 'bokheim:history:central-asia'
 -- Russia-specific local history, including Siberia.
 WHEN start_no>=510   AND end_no<=651.9999 THEN 'unified:55b70b6c-2304-5d17-80d5-c4c687b80d10'
 WHEN start_no>=751   AND end_no<=781.9999 THEN 'unified:55b70b6c-2304-5d17-80d5-c4c687b80d10'
 -- Broad and cross-republic DK spans cannot truthfully be assigned to one country.
 ELSE 'bokheim:history:soviet-union-former-republics'
 END
FROM parsed;

INSERT INTO source_selector(concept_id,system_id,selector,ordinal)
SELECT a.target_id,'lcc',a.selector,
       COALESCE((SELECT max(s.ordinal)+1 FROM source_selector s
                 WHERE s.concept_id=a.target_id AND s.system_id='lcc'),0)
       + row_number() OVER (PARTITION BY a.target_id ORDER BY a.selector) - 1
FROM dk_assignment a
WHERE NOT EXISTS (
 SELECT 1 FROM source_selector s
 WHERE s.concept_id=a.target_id AND s.system_id='lcc' AND s.selector=a.selector
);

DELETE FROM source_selector
WHERE concept_id='unified:49ac1f3c-ff3b-5d4a-b864-83cd3bf050ec'
  AND system_id='lcc'
  AND selector IN (SELECT selector FROM dk_assignment);

DROP TABLE dk_assignment;
COMMIT;

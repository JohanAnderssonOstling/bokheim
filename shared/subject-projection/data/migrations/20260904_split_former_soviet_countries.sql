-- Split Russia & the Former Soviet Union into country entries under Eastern
-- Europe and Asia. Remove Eastern European local-history detail; master stays
-- complete.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

CREATE TEMP TABLE country_merge(source_id TEXT PRIMARY KEY,target_id TEXT NOT NULL) WITHOUT ROWID;
INSERT INTO country_merge VALUES
 ('bokheim:history-lcc-full:fd5d463f-2bcc-52e4-bb69-aa23ea799b4b','unified:55b70b6c-2304-5d17-80d5-c4c687b80d10'),
 ('bokheim:history-lcc-full:6a247977-370f-5986-9585-5425a5f29018','unified:20a61563-2ae2-55d0-ac1a-11e7a0f7c4c4'),
 ('bokheim:history-lcc-full:61809d12-58df-5547-96b4-fbf463ad7fd0','unified:bab49688-fa63-5c8d-89d7-2a2dea66f052'),
 ('bokheim:history-lcc-full:f19849a1-9d05-58c8-99ad-9bf192694cc9','unified:cad75fff-9e45-5455-bab5-97baa628746d'),
 ('bokheim:history-lcc-full:1a039d2e-def1-5832-a310-b78fae6114ec','unified:1b6eac11-c24a-5110-93a1-5a55ba8d4369'),
 ('bokheim:history-lcc-full:1a1226aa-afb1-52b2-a33b-d8988dac9eac','bokheim:history-lcc-full:e3312677-a702-53c8-8d97-3da56ffc2389'),
 ('unified:18932d47-aa70-5b2f-a43c-5db96ec0a7f9','bokheim:history-lcc-full:e3312677-a702-53c8-8d97-3da56ffc2389'),
 ('bokheim:history-lcc-full:4b2aa995-57af-51e7-a5b8-a006fdbb4a7f','unified:18488bf6-5580-5533-bb8e-8bbaaab69cdf'),
 ('bokheim:history-lcc-full:53e4da73-cf0c-50b8-84d3-3a65a0605bb0','unified:c779ab27-d009-55d2-9ed3-5e63e3773683'),
 ('bokheim:history-lcc-full:03e62d5d-dbb6-540f-84cc-d33d8a172275','unified:2c6b0df0-0f86-58ea-a360-c7970f56ed8b'),
 ('bokheim:history-lcc-full:e12e37fe-fed4-5886-a29b-2676b525b1ef','unified:adac33a9-57da-55f4-a4e4-39458722dc4a'),
 ('bokheim:history-lcc-full:a527b5d8-c0da-5469-9c69-8411d46bd69c','unified:da0a14bc-4516-5ce9-8a94-8e1afae7bfe4'),
 ('bokheim:history-lcc-full:787e433b-e05e-50d1-b616-b9e5f232a84a','unified:fd48f946-9e94-5be5-ad45-026e852e21ab');

INSERT INTO source_selector(concept_id,system_id,selector,ordinal)
SELECT merge.target_id,source.system_id,source.selector,
 COALESCE((SELECT max(ordinal) FROM source_selector WHERE concept_id=merge.target_id AND system_id=source.system_id),-1)
 + row_number() OVER(PARTITION BY merge.target_id,source.system_id ORDER BY merge.source_id,source.ordinal)
FROM country_merge merge JOIN source_selector source ON source.concept_id=merge.source_id
WHERE NOT EXISTS(SELECT 1 FROM source_selector existing WHERE existing.concept_id=merge.target_id AND existing.system_id=source.system_id AND existing.selector=source.selector);

INSERT INTO source_label(concept_id,label,ordinal)
SELECT merge.target_id,source.label,
 COALESCE((SELECT max(ordinal) FROM source_label WHERE concept_id=merge.target_id),-1)
 + row_number() OVER(PARTITION BY merge.target_id ORDER BY merge.source_id,source.ordinal)
FROM country_merge merge JOIN source_label source ON source.concept_id=merge.source_id
WHERE NOT EXISTS(SELECT 1 FROM source_label existing WHERE existing.concept_id=merge.target_id AND existing.label=source.label);

CREATE TEMP TABLE promoted_country(concept_id TEXT PRIMARY KEY,target_parent TEXT NOT NULL,display_order INTEGER NOT NULL) WITHOUT ROWID;
INSERT INTO promoted_country VALUES
 ('unified:55b70b6c-2304-5d17-80d5-c4c687b80d10','unified:49ac1f3c-ff3b-5d4a-b864-83cd3bf050ec',100),
 ('unified:20a61563-2ae2-55d0-ac1a-11e7a0f7c4c4','unified:49ac1f3c-ff3b-5d4a-b864-83cd3bf050ec',110),
 ('unified:bab49688-fa63-5c8d-89d7-2a2dea66f052','unified:49ac1f3c-ff3b-5d4a-b864-83cd3bf050ec',120),
 ('unified:cad75fff-9e45-5455-bab5-97baa628746d','unified:49ac1f3c-ff3b-5d4a-b864-83cd3bf050ec',130),
 ('bokheim:history-lcc-full:1f169c2e-a1e7-51ef-8de2-ecbceb7c6ce6','unified:49ac1f3c-ff3b-5d4a-b864-83cd3bf050ec',140),
 ('bokheim:history-lcc-full:26912220-a2fa-548e-a7cb-521cd99cd7ea','unified:49ac1f3c-ff3b-5d4a-b864-83cd3bf050ec',150),
 ('bokheim:history-lcc-full:81349409-31c6-5d92-9e9a-a71164000e3a','unified:49ac1f3c-ff3b-5d4a-b864-83cd3bf050ec',160),
 ('bokheim:history-lcc-full:e3312677-a702-53c8-8d97-3da56ffc2389','bokheim:subject:history:asia',100),
 ('unified:18488bf6-5580-5533-bb8e-8bbaaab69cdf','bokheim:subject:history:asia',110),
 ('unified:1b6eac11-c24a-5110-93a1-5a55ba8d4369','bokheim:subject:history:asia',120),
 ('unified:c779ab27-d009-55d2-9ed3-5e63e3773683','bokheim:subject:history:asia',130),
 ('unified:2c6b0df0-0f86-58ea-a360-c7970f56ed8b','bokheim:subject:history:asia',140),
 ('unified:adac33a9-57da-55f4-a4e4-39458722dc4a','bokheim:subject:history:asia',150),
 ('unified:da0a14bc-4516-5ce9-8a94-8e1afae7bfe4','bokheim:subject:history:asia',160),
 ('unified:fd48f946-9e94-5be5-ad45-026e852e21ab','bokheim:subject:history:asia',170);

CREATE TEMP TABLE former_soviet_tree(concept_id TEXT PRIMARY KEY) WITHOUT ROWID;
WITH RECURSIVE tree(concept_id) AS (
 VALUES ('unified:7dfc2889-a134-5a00-9ee2-e019655fd2bb')
 UNION SELECT child.concept_id FROM concept_parent child JOIN tree parent ON parent.concept_id=child.parent_concept_id
)
INSERT INTO former_soviet_tree SELECT concept_id FROM tree;

DELETE FROM concept_parent
WHERE concept_id IN (SELECT concept_id FROM promoted_country)
  AND parent_concept_id IN (SELECT concept_id FROM former_soviet_tree);

INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal,display_order)
SELECT country.concept_id,country.target_parent,
 COALESCE((SELECT max(ordinal)+1 FROM concept_parent WHERE concept_id=country.concept_id),0),country.display_order
FROM promoted_country country;

CREATE TEMP TABLE former_soviet_remove(concept_id TEXT PRIMARY KEY) WITHOUT ROWID;
INSERT INTO former_soviet_remove SELECT concept_id FROM former_soviet_tree
WHERE concept_id NOT IN (SELECT concept_id FROM promoted_country);
DELETE FROM concept_parent
WHERE concept_id IN (SELECT concept_id FROM former_soviet_remove)
   OR parent_concept_id IN (SELECT concept_id FROM former_soviet_remove);
DELETE FROM concept WHERE concept_id IN (SELECT concept_id FROM former_soviet_remove);

UPDATE concept SET preferred_label=CASE concept_id
 WHEN 'unified:55b70b6c-2304-5d17-80d5-c4c687b80d10' THEN 'Russia'
 WHEN 'unified:20a61563-2ae2-55d0-ac1a-11e7a0f7c4c4' THEN 'Belarus'
 WHEN 'unified:bab49688-fa63-5c8d-89d7-2a2dea66f052' THEN 'Moldova'
 WHEN 'unified:cad75fff-9e45-5455-bab5-97baa628746d' THEN 'Ukraine'
 WHEN 'unified:18488bf6-5580-5533-bb8e-8bbaaab69cdf' THEN 'Azerbaijan'
 WHEN 'unified:1b6eac11-c24a-5110-93a1-5a55ba8d4369' THEN 'Georgia'
 WHEN 'bokheim:history-lcc-full:e3312677-a702-53c8-8d97-3da56ffc2389' THEN 'Armenia'
 WHEN 'unified:c779ab27-d009-55d2-9ed3-5e63e3773683' THEN 'Kazakhstan'
 WHEN 'unified:2c6b0df0-0f86-58ea-a360-c7970f56ed8b' THEN 'Kyrgyzstan'
 WHEN 'unified:adac33a9-57da-55f4-a4e4-39458722dc4a' THEN 'Tajikistan'
 WHEN 'unified:da0a14bc-4516-5ce9-8a94-8e1afae7bfe4' THEN 'Turkmenistan'
 WHEN 'unified:fd48f946-9e94-5be5-ad45-026e852e21ab' THEN 'Uzbekistan'
 ELSE preferred_label END
WHERE concept_id IN (SELECT concept_id FROM promoted_country);

-- Remove any other Local history and description branch still reachable from
-- Eastern Europe.
CREATE TEMP TABLE eastern_local_detail(concept_id TEXT PRIMARY KEY) WITHOUT ROWID;
WITH RECURSIVE
eastern(concept_id) AS (
 VALUES ('unified:49ac1f3c-ff3b-5d4a-b864-83cd3bf050ec')
 UNION SELECT child.concept_id FROM concept_parent child JOIN eastern parent ON parent.concept_id=child.parent_concept_id
),
detail(concept_id) AS (
 SELECT item.concept_id FROM eastern item JOIN concept c ON c.concept_id=item.concept_id
 WHERE lower(c.preferred_label) LIKE 'local history and description%'
 UNION SELECT child.concept_id FROM concept_parent child JOIN detail parent ON parent.concept_id=child.parent_concept_id
)
INSERT INTO eastern_local_detail SELECT concept_id FROM detail;
DELETE FROM concept_parent
WHERE concept_id IN (SELECT concept_id FROM eastern_local_detail)
   OR parent_concept_id IN (SELECT concept_id FROM eastern_local_detail);
DELETE FROM concept WHERE concept_id IN (SELECT concept_id FROM eastern_local_detail);

DROP TABLE eastern_local_detail;
DROP TABLE former_soviet_remove;
DROP TABLE former_soviet_tree;
DROP TABLE promoted_country;
DROP TABLE country_merge;
COMMIT;

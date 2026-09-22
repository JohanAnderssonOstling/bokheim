-- Restore country-specific periods for countries split out of the former
-- Soviet aggregate. Period wrappers and locality/topical detail stay removed.

PRAGMA foreign_keys = ON;
ATTACH DATABASE 'master-taxonomy-v2.sqlite3' AS master;
BEGIN IMMEDIATE;

CREATE TEMP TABLE period_source(source_id TEXT PRIMARY KEY,target_id TEXT NOT NULL) WITHOUT ROWID;
INSERT INTO period_source VALUES
 ('bokheim:history-lcc-full:fd5d463f-2bcc-52e4-bb69-aa23ea799b4b','unified:55b70b6c-2304-5d17-80d5-c4c687b80d10'),
 ('bokheim:history-lcc-full:6a247977-370f-5986-9585-5425a5f29018','unified:20a61563-2ae2-55d0-ac1a-11e7a0f7c4c4'),
 ('bokheim:history-lcc-full:61809d12-58df-5547-96b4-fbf463ad7fd0','unified:bab49688-fa63-5c8d-89d7-2a2dea66f052'),
 ('unified:cad75fff-9e45-5455-bab5-97baa628746d','unified:cad75fff-9e45-5455-bab5-97baa628746d'),
 ('bokheim:history-lcc-full:1f169c2e-a1e7-51ef-8de2-ecbceb7c6ce6','bokheim:history-lcc-full:1f169c2e-a1e7-51ef-8de2-ecbceb7c6ce6'),
 ('bokheim:history-lcc-full:26912220-a2fa-548e-a7cb-521cd99cd7ea','bokheim:history-lcc-full:26912220-a2fa-548e-a7cb-521cd99cd7ea'),
 ('bokheim:history-lcc-full:81349409-31c6-5d92-9e9a-a71164000e3a','bokheim:history-lcc-full:81349409-31c6-5d92-9e9a-a71164000e3a'),
 ('bokheim:history-lcc-full:1a1226aa-afb1-52b2-a33b-d8988dac9eac','bokheim:history-lcc-full:e3312677-a702-53c8-8d97-3da56ffc2389'),
 ('bokheim:history-lcc-full:4b2aa995-57af-51e7-a5b8-a006fdbb4a7f','unified:18488bf6-5580-5533-bb8e-8bbaaab69cdf'),
 ('bokheim:history-lcc-full:1a039d2e-def1-5832-a310-b78fae6114ec','unified:1b6eac11-c24a-5110-93a1-5a55ba8d4369'),
 ('bokheim:history-lcc-full:53e4da73-cf0c-50b8-84d3-3a65a0605bb0','unified:c779ab27-d009-55d2-9ed3-5e63e3773683'),
 ('bokheim:history-lcc-full:03e62d5d-dbb6-540f-84cc-d33d8a172275','unified:2c6b0df0-0f86-58ea-a360-c7970f56ed8b'),
 ('bokheim:history-lcc-full:e12e37fe-fed4-5886-a29b-2676b525b1ef','unified:adac33a9-57da-55f4-a4e4-39458722dc4a'),
 ('bokheim:history-lcc-full:a527b5d8-c0da-5469-9c69-8411d46bd69c','unified:da0a14bc-4516-5ce9-8a94-8e1afae7bfe4'),
 ('bokheim:history-lcc-full:787e433b-e05e-50d1-b616-b9e5f232a84a','unified:fd48f946-9e94-5be5-ad45-026e852e21ab');

-- Clear current country detail (notably Ukraine's topical branch) before
-- rebuilding only periods.
CREATE TEMP TABLE current_detail(concept_id TEXT PRIMARY KEY) WITHOUT ROWID;
WITH RECURSIVE detail(concept_id) AS (
 SELECT child.concept_id FROM period_source country
 JOIN concept_parent child ON child.parent_concept_id=country.target_id
 UNION
 SELECT child.concept_id FROM detail parent
 JOIN concept_parent child ON child.parent_concept_id=parent.concept_id
)
INSERT INTO current_detail SELECT concept_id FROM detail;
DELETE FROM concept_parent
WHERE concept_id IN (SELECT concept_id FROM current_detail)
   OR parent_concept_id IN (SELECT concept_id FROM current_detail);
DELETE FROM concept WHERE concept_id IN (SELECT concept_id FROM current_detail);

CREATE TEMP TABLE country_period(
 target_id TEXT NOT NULL,
 concept_id TEXT PRIMARY KEY,
 display_order INTEGER NOT NULL
) WITHOUT ROWID;
INSERT OR IGNORE INTO country_period
SELECT source.target_id,period.concept_id,period.display_order
FROM period_source source
JOIN master.concept_parent wrapper ON wrapper.parent_concept_id=source.source_id
JOIN master.concept wrapper_item ON wrapper_item.concept_id=wrapper.concept_id
JOIN master.concept_parent period ON period.parent_concept_id=wrapper.concept_id
WHERE lower(wrapper_item.preferred_label)='by period';

INSERT OR IGNORE INTO country_period
SELECT source.target_id,child.concept_id,10000+child.display_order
FROM period_source source
JOIN master.concept_parent child ON child.parent_concept_id=source.source_id
JOIN master.concept item ON item.concept_id=child.concept_id
WHERE item.preferred_label GLOB '[0-9]*'
   OR lower(item.preferred_label) GLOB 'early*'
   OR lower(item.preferred_label) GLOB 'through *';

INSERT OR IGNORE INTO concept(concept_id,preferred_label)
SELECT source.concept_id,source.preferred_label FROM master.concept source
JOIN country_period period ON period.concept_id=source.concept_id;
INSERT OR IGNORE INTO source_label(concept_id,label,ordinal)
SELECT source.concept_id,source.label,source.ordinal FROM master.source_label source
JOIN country_period period ON period.concept_id=source.concept_id;
INSERT OR IGNORE INTO source_selector(concept_id,system_id,selector,ordinal)
SELECT source.concept_id,source.system_id,source.selector,source.ordinal FROM master.source_selector source
JOIN country_period period ON period.concept_id=source.concept_id;
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal,display_order)
SELECT period.concept_id,period.target_id,0,period.display_order FROM country_period period;

DROP TABLE country_period;
DROP TABLE current_detail;
DROP TABLE period_source;
COMMIT;
DETACH DATABASE master;

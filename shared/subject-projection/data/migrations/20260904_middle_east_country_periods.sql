-- Rebuild curated Middle Eastern country branches as periods only. Regional
-- subdivisions and generic topical detail remain available in the master.

PRAGMA foreign_keys = ON;
ATTACH DATABASE 'master-taxonomy-v2.sqlite3' AS master;
BEGIN IMMEDIATE;

CREATE TEMP TABLE period_source(source_id TEXT PRIMARY KEY,target_id TEXT NOT NULL) WITHOUT ROWID;
INSERT INTO period_source VALUES
 ('unified:0756cf48-f424-58af-b65f-94d14994861d','unified:0756cf48-f424-58af-b65f-94d14994861d'),
 ('unified:e39978ec-a73a-5949-8c89-f2f413386e3e','unified:e39978ec-a73a-5949-8c89-f2f413386e3e'),
 ('unified:34ad48ff-92de-5584-8307-c2205c6e0f5d','unified:34ad48ff-92de-5584-8307-c2205c6e0f5d'),
 ('unified:51d2665b-797b-5354-9470-a36fa9e65fb9','unified:51d2665b-797b-5354-9470-a36fa9e65fb9'),
 ('unified:2f776a9d-cdd5-5ada-b28c-888db2077187','unified:2f776a9d-cdd5-5ada-b28c-888db2077187'),
 ('unified:356028f8-c8cd-54c1-879e-2af2da6a204c','unified:356028f8-c8cd-54c1-879e-2af2da6a204c'),
 ('unified:e37e872e-1221-57a6-b5f9-74a307f67b3d','unified:e37e872e-1221-57a6-b5f9-74a307f67b3d'),
 ('unified:e846c8e2-891e-51b4-adf6-eae0842628eb','unified:e846c8e2-891e-51b4-adf6-eae0842628eb'),
 ('unified:b1da6c49-e4e6-5dca-b82a-8f37a1793e80','unified:b1da6c49-e4e6-5dca-b82a-8f37a1793e80'),
 ('unified:3d37af1e-3f24-581f-8e49-ca78250004b2','unified:3d37af1e-3f24-581f-8e49-ca78250004b2');
DELETE FROM period_source WHERE target_id NOT IN (SELECT concept_id FROM concept);

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
WHERE parent_concept_id IN (SELECT target_id FROM period_source)
  AND concept_id IN (SELECT concept_id FROM current_detail);
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
   OR lower(item.preferred_label) GLOB 'earliest*'
   OR lower(item.preferred_label) GLOB 'ancient*'
   OR lower(item.preferred_label) GLOB 'through *'
   OR lower(item.preferred_label) GLOB 'republic,*';

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
SELECT period.concept_id,period.target_id,
       COALESCE((SELECT max(ordinal)+1 FROM concept_parent WHERE concept_id=period.concept_id),0),
       period.display_order
FROM country_period period;

DROP TABLE country_period;
DROP TABLE current_detail;
DROP TABLE period_source;
COMMIT;
DETACH DATABASE master;

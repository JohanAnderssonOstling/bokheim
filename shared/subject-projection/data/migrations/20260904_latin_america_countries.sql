-- Keep only sovereign-country entries beneath the curated Latin America node.
-- Detailed geography and historical subdivisions remain in the master database.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

CREATE TEMP TABLE latin_country(
    concept_id TEXT PRIMARY KEY,
    preferred_label TEXT NOT NULL,
    display_order INTEGER NOT NULL
) WITHOUT ROWID;
INSERT INTO latin_country VALUES
    ('bokheim:history-lcc-full:77d6cba6-921d-5aba-b601-4a74c440a095', 'Argentina', 10),
    ('unified:33f827a7-71e3-51d2-bc95-7703fac9de8f', 'Bahamas', 20),
    ('unified:5f96244e-03fd-531f-abf1-785a4e0b9f8b', 'Belize', 30),
    ('unified:2eedfccb-541c-59a9-9d1e-d0851a041a6a', 'Bolivia', 40),
    ('unified:650a7519-0b1e-530e-99a5-a044b1481085', 'Brazil', 50),
    ('unified:2008bb1e-e4d6-5cc4-86a9-fb613255bce7', 'Chile', 60),
    ('bokheim:subject:history:americas:south-america:colombia', 'Colombia', 70),
    ('unified:f5fc95c1-26fd-5855-bda2-6f8acd8e3993', 'Costa Rica', 80),
    ('unified:72a34e3a-79e3-520f-a9bd-ae2ddb0affbb', 'Cuba', 90),
    ('unified:8e7e075b-2d13-53ad-a349-a1a056d96ff2', 'Dominican Republic', 100),
    ('bokheim:subject:history:americas:south-america:ecuador', 'Ecuador', 110),
    ('unified:9d663504-19de-51f7-b54d-f3d5311a3cb2', 'El Salvador', 120),
    ('unified:267fdca5-98e6-5f1c-8ad6-bf5f69e762ec', 'Guatemala', 130),
    ('bokheim:history-lcc-full:3327c795-9301-5d7c-978a-4194fd9cc7f9', 'Guyana', 140),
    ('unified:55c8532e-37bf-521f-8ade-d62aa7c34138', 'Haiti', 150),
    ('unified:cad9ac35-6434-551c-9da3-3034adbf2c89', 'Honduras', 160),
    ('unified:c2541453-e123-532e-94a4-30c8c4238f76', 'Jamaica', 170),
    ('unified:4eb4c583-bd48-5099-a1f6-dd54e5eaaae6', 'Mexico', 180),
    ('unified:b3750167-22c8-5027-b784-702166c900ff', 'Nicaragua', 190),
    ('unified:f18ea268-7072-5df4-b5cd-623a76dd2399', 'Panama', 200),
    ('bokheim:subject:history:americas:south-america:paraguay', 'Paraguay', 210),
    ('unified:ce1ee54b-9abc-51ad-a2c8-7faea3e507a7', 'Peru', 220),
    ('bokheim:history-lcc-full:a55b19ef-2ad7-5837-a6a1-64a605cf72c9', 'Suriname', 230),
    ('bokheim:subject:history:americas:south-america:uruguay', 'Uruguay', 240),
    ('unified:194cb7f2-6b85-5003-b481-9c3e503c6921', 'Venezuela', 250);

CREATE TEMP TABLE latin_detail(concept_id TEXT PRIMARY KEY) WITHOUT ROWID;
WITH RECURSIVE subtree(concept_id) AS (
    VALUES ('unified:1934d328-cf65-57fa-95b1-1c7e93cf34c3')
    UNION
    SELECT child.concept_id
    FROM concept_parent child
    JOIN subtree parent ON parent.concept_id = child.parent_concept_id
)
INSERT INTO latin_detail
SELECT concept_id FROM subtree
WHERE concept_id <> 'unified:1934d328-cf65-57fa-95b1-1c7e93cf34c3'
  AND concept_id NOT IN (SELECT concept_id FROM latin_country);

CREATE TEMP TABLE latin_country_merge(source_id TEXT PRIMARY KEY, target_id TEXT NOT NULL) WITHOUT ROWID;
INSERT INTO latin_country_merge VALUES
    ('bokheim:history-lcc-full:c0f1dbaa-79ab-5a5c-965b-4c6483e37171', 'unified:5f96244e-03fd-531f-abf1-785a4e0b9f8b'),
    ('bokheim:history-lcc:2f3a6960-9a0e-59d2-b48d-3009252bd7f6', 'unified:72a34e3a-79e3-520f-a9bd-ae2ddb0affbb'),
    ('bokheim:history-lcc:48cd88d0-7ada-59d7-a5b4-b59bf703341f', 'unified:c2541453-e123-532e-94a4-30c8c4238f76'),
    ('bokheim:history-lcc-full:24a05056-44af-5391-9392-b962136ca0d4', 'unified:f18ea268-7072-5df4-b5cd-623a76dd2399');

INSERT INTO source_selector(concept_id, system_id, selector, ordinal)
SELECT merge.target_id, source.system_id, source.selector,
       COALESCE((SELECT max(ordinal) FROM source_selector WHERE concept_id=merge.target_id AND system_id=source.system_id), -1)
       + row_number() OVER (PARTITION BY merge.target_id, source.system_id ORDER BY merge.source_id, source.ordinal)
FROM latin_country_merge merge
JOIN source_selector source ON source.concept_id=merge.source_id
WHERE NOT EXISTS (
    SELECT 1 FROM source_selector existing
    WHERE existing.concept_id=merge.target_id
      AND existing.system_id=source.system_id
      AND existing.selector=source.selector
);

INSERT INTO source_label(concept_id, label, ordinal)
SELECT merge.target_id, source.label,
       COALESCE((SELECT max(ordinal) FROM source_label WHERE concept_id=merge.target_id), -1)
       + row_number() OVER (PARTITION BY merge.target_id ORDER BY merge.source_id, source.ordinal)
FROM latin_country_merge merge
JOIN source_label source ON source.concept_id=merge.source_id
WHERE NOT EXISTS (
    SELECT 1 FROM source_label existing
    WHERE existing.concept_id=merge.target_id AND existing.label=source.label
);

-- Remove the old paths within Latin America, then attach the retained countries
-- directly. Existing parents outside this branch, if any, remain intact.
DELETE FROM concept_parent
WHERE concept_id IN (SELECT concept_id FROM latin_country)
  AND parent_concept_id IN (
      SELECT concept_id FROM latin_detail
      UNION SELECT 'unified:1934d328-cf65-57fa-95b1-1c7e93cf34c3'
  );

INSERT INTO concept_parent(concept_id, parent_concept_id, ordinal, display_order)
SELECT country.concept_id,
       'unified:1934d328-cf65-57fa-95b1-1c7e93cf34c3',
       COALESCE((SELECT max(ordinal) + 1 FROM concept_parent WHERE concept_id=country.concept_id), 0),
       country.display_order
FROM latin_country country;

UPDATE concept
SET preferred_label=(SELECT preferred_label FROM latin_country WHERE latin_country.concept_id=concept.concept_id)
WHERE concept_id IN (SELECT concept_id FROM latin_country);

DELETE FROM concept_parent
WHERE concept_id IN (SELECT concept_id FROM latin_detail)
   OR parent_concept_id IN (SELECT concept_id FROM latin_detail);
DELETE FROM concept WHERE concept_id IN (SELECT concept_id FROM latin_detail);

DROP TABLE latin_country_merge;
DROP TABLE latin_detail;
DROP TABLE latin_country;
COMMIT;

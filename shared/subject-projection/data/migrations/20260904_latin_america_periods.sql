-- Restore only historical periods beneath curated Latin American countries.
-- Periods are attached directly to countries; generic "By period" wrappers
-- and all geographic/topical detail remain excluded from the curated graph.
-- Run from the data directory so the master database can be attached by name.

PRAGMA foreign_keys = ON;
ATTACH DATABASE 'master-taxonomy-v2.sqlite3' AS master;
BEGIN IMMEDIATE;

CREATE TEMP TABLE latin_country(concept_id TEXT PRIMARY KEY) WITHOUT ROWID;
INSERT INTO latin_country VALUES
    ('bokheim:history-lcc-full:77d6cba6-921d-5aba-b601-4a74c440a095'),
    ('unified:33f827a7-71e3-51d2-bc95-7703fac9de8f'),
    ('unified:5f96244e-03fd-531f-abf1-785a4e0b9f8b'),
    ('unified:2eedfccb-541c-59a9-9d1e-d0851a041a6a'),
    ('unified:650a7519-0b1e-530e-99a5-a044b1481085'),
    ('unified:2008bb1e-e4d6-5cc4-86a9-fb613255bce7'),
    ('bokheim:subject:history:americas:south-america:colombia'),
    ('unified:f5fc95c1-26fd-5855-bda2-6f8acd8e3993'),
    ('unified:72a34e3a-79e3-520f-a9bd-ae2ddb0affbb'),
    ('unified:8e7e075b-2d13-53ad-a349-a1a056d96ff2'),
    ('bokheim:subject:history:americas:south-america:ecuador'),
    ('unified:9d663504-19de-51f7-b54d-f3d5311a3cb2'),
    ('unified:267fdca5-98e6-5f1c-8ad6-bf5f69e762ec'),
    ('bokheim:history-lcc-full:3327c795-9301-5d7c-978a-4194fd9cc7f9'),
    ('unified:55c8532e-37bf-521f-8ade-d62aa7c34138'),
    ('unified:cad9ac35-6434-551c-9da3-3034adbf2c89'),
    ('unified:c2541453-e123-532e-94a4-30c8c4238f76'),
    ('unified:4eb4c583-bd48-5099-a1f6-dd54e5eaaae6'),
    ('unified:b3750167-22c8-5027-b784-702166c900ff'),
    ('unified:f18ea268-7072-5df4-b5cd-623a76dd2399'),
    ('bokheim:subject:history:americas:south-america:paraguay'),
    ('unified:ce1ee54b-9abc-51ad-a2c8-7faea3e507a7'),
    ('bokheim:history-lcc-full:a55b19ef-2ad7-5837-a6a1-64a605cf72c9'),
    ('bokheim:subject:history:americas:south-america:uruguay'),
    ('unified:194cb7f2-6b85-5003-b481-9c3e503c6921');

CREATE TEMP TABLE latin_period(
    country_id TEXT NOT NULL,
    concept_id TEXT PRIMARY KEY,
    display_order INTEGER NOT NULL
) WITHOUT ROWID;

-- Immediate children of each country's primary By period grouping.
INSERT OR IGNORE INTO latin_period
SELECT country.concept_id, period.concept_id, period.display_order
FROM latin_country country
JOIN master.concept_parent wrapper ON wrapper.parent_concept_id=country.concept_id
JOIN master.concept wrapper_concept ON wrapper_concept.concept_id=wrapper.concept_id
JOIN master.concept_parent period ON period.parent_concept_id=wrapper.concept_id
WHERE lower(wrapper_concept.preferred_label)='by period';

-- Some source hierarchies place a chronological entry directly under the
-- country (Mexico's 2000- is one example).
INSERT OR IGNORE INTO latin_period
SELECT country.concept_id, period.concept_id, 10000 + period.display_order
FROM latin_country country
JOIN master.concept_parent period ON period.parent_concept_id=country.concept_id
JOIN master.concept period_concept ON period_concept.concept_id=period.concept_id
WHERE period_concept.preferred_label GLOB '[0-9]*';

INSERT OR IGNORE INTO concept(concept_id, preferred_label)
SELECT source.concept_id, source.preferred_label
FROM master.concept source
JOIN latin_period period ON period.concept_id=source.concept_id;

INSERT OR IGNORE INTO source_label(concept_id, label, ordinal)
SELECT source.concept_id, source.label, source.ordinal
FROM master.source_label source
JOIN latin_period period ON period.concept_id=source.concept_id;

INSERT OR IGNORE INTO source_selector(concept_id, system_id, selector, ordinal)
SELECT source.concept_id, source.system_id, source.selector, source.ordinal
FROM master.source_selector source
JOIN latin_period period ON period.concept_id=source.concept_id;

INSERT OR IGNORE INTO concept_parent(concept_id, parent_concept_id, ordinal, display_order)
SELECT period.concept_id,
       period.country_id,
       COALESCE((SELECT max(ordinal) + 1 FROM concept_parent WHERE concept_id=period.concept_id), 0),
       period.display_order
FROM latin_period period;

DROP TABLE latin_period;
DROP TABLE latin_country;
COMMIT;
DETACH DATABASE master;

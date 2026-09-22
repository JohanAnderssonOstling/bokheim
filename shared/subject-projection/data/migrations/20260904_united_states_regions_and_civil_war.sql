-- Consolidate parallel United States regional roots into the four broad
-- regions, and use Historical Eras > Civil War Period as the sole curated
-- United States Civil War concept. Full detail remains in the master database.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

CREATE TEMP TABLE regional_merge(source_id TEXT PRIMARY KEY, target_id TEXT NOT NULL) WITHOUT ROWID;
INSERT INTO regional_merge VALUES
    ('bokheim:history-lcc-full:54b74e23-da3e-54fc-a578-51f31ce8c549', 'unified:bf161ac8-5880-5d79-8e94-8d3f6c34046e'),
    ('bokheim:history-lcc-full:55ba5f18-389c-5447-951a-327a953f0deb', 'unified:508a9bbd-bc3f-50fe-97f7-d1c27c8aacdc'),
    ('bokheim:history-lcc-full:9a547510-44dc-59f1-b4f9-45c35b811a46', 'unified:508a9bbd-bc3f-50fe-97f7-d1c27c8aacdc'),
    ('bokheim:history-lcc-full:eaff3a15-f574-5e60-a8cb-118e45107f28', 'unified:413b2289-3f2f-5000-8218-a8bca93ba70b'),
    ('bokheim:history-lcc-full:56682271-eb1b-5ea8-acb7-b1d24b7d3eaa', 'unified:413b2289-3f2f-5000-8218-a8bca93ba70b'),
    ('bokheim:history-lcc-full:c586852f-121c-534b-8817-64081e94fda8', 'unified:67e96244-92a5-55e7-8801-c05d8911dea6'),
    ('bokheim:history-lcc-full:0306b053-5ce6-57ea-9fae-1f2265e8e3c3', 'unified:67e96244-92a5-55e7-8801-c05d8911dea6'),
    ('bokheim:history-lcc-full:81e6281c-f373-524f-8c9e-6958af5a0cad', 'unified:67e96244-92a5-55e7-8801-c05d8911dea6'),
    ('bokheim:history-lcc-full:4bab1560-5a8e-5720-bcdf-9826f9a007a0', 'unified:67e96244-92a5-55e7-8801-c05d8911dea6'),
    ('bokheim:history-lcc-full:c8883acc-e74a-5e27-9bbc-f22be2fd8b76', 'unified:67e96244-92a5-55e7-8801-c05d8911dea6');

INSERT INTO source_selector(concept_id,system_id,selector,ordinal)
SELECT merge.target_id, source.system_id, source.selector,
       COALESCE((SELECT max(ordinal) FROM source_selector WHERE concept_id=merge.target_id AND system_id=source.system_id),-1)
       + row_number() OVER(PARTITION BY merge.target_id,source.system_id ORDER BY merge.source_id,source.ordinal)
FROM regional_merge merge JOIN source_selector source ON source.concept_id=merge.source_id
WHERE NOT EXISTS(SELECT 1 FROM source_selector existing WHERE existing.concept_id=merge.target_id AND existing.system_id=source.system_id AND existing.selector=source.selector);

CREATE TEMP TABLE curated_detail_to_remove(concept_id TEXT PRIMARY KEY) WITHOUT ROWID;
WITH RECURSIVE removed(concept_id) AS (
    SELECT source_id FROM regional_merge
    UNION
    SELECT child.concept_id FROM concept_parent child JOIN removed parent ON parent.concept_id=child.parent_concept_id
)
INSERT INTO curated_detail_to_remove SELECT concept_id FROM removed;

-- Merge all United States Civil War selectors into the historical-era node.
INSERT INTO source_selector(concept_id,system_id,selector,ordinal)
SELECT 'unified:5a4c7d22-f7b0-59cc-842c-4a16bca2b64f', source.system_id, source.selector,
       COALESCE((SELECT max(ordinal) FROM source_selector WHERE concept_id='unified:5a4c7d22-f7b0-59cc-842c-4a16bca2b64f' AND system_id=source.system_id),-1)
       + row_number() OVER(PARTITION BY source.system_id ORDER BY source.concept_id,source.ordinal)
FROM source_selector source
WHERE source.concept_id IN (
    'bokheim:history-lcc:47a07a9b-801c-5ba9-9887-0d13d9735fb2',
    'bokheim:history-lcc:0c6cd134-03f9-5590-8e4e-61cd58741a9b',
    'bokheim:history-lcc-full:fcdf0a07-bc09-58c6-8a12-19051d3ef49d'
)
AND NOT EXISTS(SELECT 1 FROM source_selector existing WHERE existing.concept_id='unified:5a4c7d22-f7b0-59cc-842c-4a16bca2b64f' AND existing.system_id=source.system_id AND existing.selector=source.selector);

WITH RECURSIVE civil_war_detail(concept_id) AS (
    VALUES
      ('bokheim:history-lcc:47a07a9b-801c-5ba9-9887-0d13d9735fb2'),
      ('bokheim:history-lcc:0c6cd134-03f9-5590-8e4e-61cd58741a9b'),
      ('bokheim:history-lcc-full:fcdf0a07-bc09-58c6-8a12-19051d3ef49d')
    UNION
    SELECT child.concept_id FROM concept_parent child JOIN civil_war_detail parent ON parent.concept_id=child.parent_concept_id
)
INSERT OR IGNORE INTO curated_detail_to_remove SELECT concept_id FROM civil_war_detail;

DELETE FROM concept_parent
WHERE concept_id IN (SELECT concept_id FROM curated_detail_to_remove)
   OR parent_concept_id IN (SELECT concept_id FROM curated_detail_to_remove);
DELETE FROM concept WHERE concept_id IN (SELECT concept_id FROM curated_detail_to_remove);

-- The canonical Civil War concept needs only one broad LCC range plus the
-- southern-history selector imported from the former regional duplicate.
DELETE FROM source_selector
WHERE concept_id='unified:5a4c7d22-f7b0-59cc-842c-4a16bca2b64f'
  AND system_id='lcc'
  AND selector NOT IN ('E456..E655@2','F214..F214@5');

DROP TABLE curated_detail_to_remove;
DROP TABLE regional_merge;
COMMIT;

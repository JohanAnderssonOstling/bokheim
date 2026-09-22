-- Consolidate U.S. periods under Historical Eras and merge equivalent era
-- concepts. Resulting eras remain leaves; full detail remains in master.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

CREATE TEMP TABLE era_merge(source_id TEXT PRIMARY KEY,target_id TEXT NOT NULL) WITHOUT ROWID;
INSERT INTO era_merge VALUES
 ('bokheim:history-lcc:2bbf1350-ad4f-5080-8b78-61aba6d60af3','unified:b895c2d6-0a1a-5025-ab01-325cfec68f0d'),
 ('bokheim:history-lcc:03214d7a-5f87-53b1-ad12-f05807c6e125','unified:62cd3138-1b83-5a2a-89c9-f9485580afde'),
 ('bokheim:history-lcc-full:37adebda-f12f-54e0-889d-287363391ee1','unified:deae860a-f708-5ec3-a9ee-5cac8c52dae4'),
 ('unified:2b3f2614-e32f-5cc6-a7b2-8a91e1b50709','unified:4b659071-417c-5122-8df1-52a974601cf7');

INSERT INTO source_label(concept_id,label,ordinal)
SELECT merge.target_id,source.label,
       COALESCE((SELECT max(ordinal) FROM source_label
                 WHERE concept_id=merge.target_id),-1)
       + row_number() OVER (PARTITION BY merge.target_id ORDER BY source.ordinal,source.label)
FROM era_merge merge
JOIN source_label source ON source.concept_id=merge.source_id
WHERE NOT EXISTS (
 SELECT 1 FROM source_label existing
 WHERE existing.concept_id=merge.target_id AND existing.label=source.label
);

INSERT INTO source_selector(concept_id,system_id,selector,ordinal)
SELECT merge.target_id,source.system_id,source.selector,
       COALESCE((SELECT max(ordinal) FROM source_selector
                 WHERE concept_id=merge.target_id AND system_id=source.system_id),-1)
       + row_number() OVER (
           PARTITION BY merge.target_id,source.system_id
           ORDER BY source.ordinal,source.selector
         )
FROM era_merge merge
JOIN source_selector source ON source.concept_id=merge.source_id
WHERE NOT EXISTS (
 SELECT 1 FROM source_selector existing
 WHERE existing.concept_id=merge.target_id
   AND existing.system_id=source.system_id
   AND existing.selector=source.selector
);

CREATE TEMP TABLE merged_era_detail(concept_id TEXT PRIMARY KEY) WITHOUT ROWID;
WITH RECURSIVE detail(concept_id) AS (
 SELECT child.concept_id
 FROM era_merge merge
 JOIN concept_parent child ON child.parent_concept_id=merge.source_id
 UNION
 SELECT child.concept_id
 FROM detail parent
 JOIN concept_parent child ON child.parent_concept_id=parent.concept_id
)
INSERT INTO merged_era_detail SELECT concept_id FROM detail;

DELETE FROM concept_parent
WHERE concept_id IN (SELECT concept_id FROM merged_era_detail)
   OR parent_concept_id IN (SELECT concept_id FROM merged_era_detail);
DELETE FROM concept WHERE concept_id IN (SELECT concept_id FROM merged_era_detail);

DELETE FROM concept_parent
WHERE concept_id IN (SELECT source_id FROM era_merge)
   OR parent_concept_id IN (SELECT source_id FROM era_merge);
DELETE FROM concept WHERE concept_id IN (SELECT source_id FROM era_merge);

DROP TABLE merged_era_detail;
DROP TABLE era_merge;
COMMIT;

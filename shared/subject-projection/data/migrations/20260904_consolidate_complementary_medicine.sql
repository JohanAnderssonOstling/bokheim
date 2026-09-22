-- Consolidate duplicate BISAC/LCC treatment systems beneath one shallow
-- Complementary & Traditional Medicine branch.

PRAGMA foreign_keys = ON;
ATTACH DATABASE 'master-taxonomy-v2.sqlite3' AS master;
BEGIN IMMEDIATE;

CREATE TEMP TABLE merge_seed(source_id TEXT PRIMARY KEY,target_id TEXT NOT NULL) WITHOUT ROWID;
INSERT INTO merge_seed VALUES
 ('unified:d067ec62-fb42-5809-bce2-1319950a4b27','unified:d067ec62-fb42-5809-bce2-1319950a4b27'),
 ('unified:aaddea61-c82b-5172-a0d6-b316fb02bec8','unified:aaddea61-c82b-5172-a0d6-b316fb02bec8'),
 ('unified:0cc0f355-9066-5a9e-8af6-34b6fa5b84c7','unified:ff47c13e-9779-5b7d-95ad-4dae9b176834'),
 ('unified:229547c2-d70b-5b47-87fe-b3277cdb81ea','unified:14d3d663-8a4c-59ea-9ef1-1d85b77e0316'),
 ('unified:8af31076-5a02-5705-b5e9-130469663bcc','unified:8af31076-5a02-5705-b5e9-130469663bcc'),
 ('unified:fa3e256e-0b4f-5758-be65-43ea2a277671','unified:fa3e256e-0b4f-5758-be65-43ea2a277671'),
 ('unified:47a4eb3f-cb1b-503c-acab-a75660b59641','unified:47a4eb3f-cb1b-503c-acab-a75660b59641');

CREATE TEMP TABLE merge_member(
 target_id TEXT NOT NULL,
 source_id TEXT NOT NULL,
 PRIMARY KEY(target_id,source_id)
) WITHOUT ROWID;
WITH RECURSIVE members(target_id,source_id) AS (
 SELECT target_id,source_id FROM merge_seed
 UNION
 SELECT members.target_id,edge.concept_id
 FROM members JOIN concept_parent edge ON edge.parent_concept_id=members.source_id
)
INSERT INTO merge_member SELECT target_id,source_id FROM members;

-- Preserve every detailed selector on the retained treatment leaf.
WITH candidates AS (
 SELECT DISTINCT member.target_id,selector.system_id,selector.selector
 FROM merge_member member
 JOIN source_selector selector ON selector.concept_id=member.source_id
 WHERE NOT EXISTS (
  SELECT 1 FROM source_selector existing
  WHERE existing.concept_id=member.target_id
    AND existing.system_id=selector.system_id
    AND existing.selector=selector.selector
 )
), numbered AS (
 SELECT candidates.*,
        row_number() OVER (PARTITION BY target_id,system_id ORDER BY selector)-1 AS offset
 FROM candidates
)
INSERT INTO source_selector(concept_id,system_id,selector,ordinal)
SELECT target_id,system_id,selector,
       COALESCE((SELECT max(existing.ordinal)+1 FROM source_selector existing
                 WHERE existing.concept_id=numbered.target_id
                   AND existing.system_id=numbered.system_id),0)+offset
FROM numbered;

-- Preserve source wording from merged concepts for search and diagnostics.
WITH candidates AS (
 SELECT DISTINCT member.target_id,label.label
 FROM merge_member member JOIN source_label label ON label.concept_id=member.source_id
 WHERE NOT EXISTS (
  SELECT 1 FROM source_label existing
  WHERE existing.concept_id=member.target_id AND existing.label=label.label
 )
), numbered AS (
 SELECT candidates.*,
        row_number() OVER (PARTITION BY target_id ORDER BY label)-1 AS offset
 FROM candidates
)
INSERT INTO source_label(concept_id,label,ordinal)
SELECT target_id,label,
       COALESCE((SELECT max(existing.ordinal)+1 FROM source_label existing
                 WHERE existing.concept_id=numbered.target_id),0)+offset
FROM numbered;

-- RZ covers the whole LCC alternative-medicine grouping, so it belongs on the
-- shared curated parent rather than on another redundant umbrella.
INSERT INTO source_selector(concept_id,system_id,selector,ordinal)
SELECT 'bokheim:subject:health:complementary',system_id,selector,ordinal
FROM source_selector
WHERE concept_id='unified:e4c4e711-1ffb-54fd-bc0e-c2469cf949c1';

-- Promote the unique branches out of Other Systems of Medicine.
UPDATE concept_parent
SET parent_concept_id='bokheim:subject:health:complementary'
WHERE parent_concept_id='unified:e4c4e711-1ffb-54fd-bc0e-c2469cf949c1'
  AND concept_id IN (
   'unified:8af31076-5a02-5705-b5e9-130469663bcc',
   'unified:fa3e256e-0b4f-5758-be65-43ea2a277671',
   'unified:47a4eb3f-cb1b-503c-acab-a75660b59641'
  );

UPDATE concept SET preferred_label='Botanical & Eclectic Medicine'
WHERE concept_id='unified:d067ec62-fb42-5809-bce2-1319950a4b27';
UPDATE concept SET preferred_label='Other Systems'
WHERE concept_id='unified:fa3e256e-0b4f-5758-be65-43ea2a277671';
UPDATE concept SET preferred_label='Osteo-Magnetics & Neuropathy'
WHERE concept_id='unified:47a4eb3f-cb1b-503c-acab-a75660b59641';

-- Every named treatment system is terminal in the curated browser.
DELETE FROM concept_parent
WHERE parent_concept_id IN (
 'unified:a9078b12-7b1d-5df5-8f5c-c15646cd459e',
 'unified:d067ec62-fb42-5809-bce2-1319950a4b27',
 'unified:ff47c13e-9779-5b7d-95ad-4dae9b176834',
 'unified:334c00c6-b853-52da-a2ea-3d21bfd2c0cd',
 'unified:77f7582d-384a-534f-a87f-3d6c4f2eb351',
 'unified:aaddea61-c82b-5172-a0d6-b316fb02bec8',
 'unified:14d3d663-8a4c-59ea-9ef1-1d85b77e0316',
 'unified:8af31076-5a02-5705-b5e9-130469663bcc',
 'unified:fa3e256e-0b4f-5758-be65-43ea2a277671',
 'unified:47a4eb3f-cb1b-503c-acab-a75660b59641'
);

DELETE FROM concept_parent WHERE concept_id='unified:e4c4e711-1ffb-54fd-bc0e-c2469cf949c1';
DELETE FROM concept_parent WHERE parent_concept_id='unified:e4c4e711-1ffb-54fd-bc0e-c2469cf949c1';
DELETE FROM concept WHERE concept_id='unified:e4c4e711-1ffb-54fd-bc0e-c2469cf949c1';

CREATE TEMP TABLE valid_root(concept_id TEXT PRIMARY KEY) WITHOUT ROWID;
INSERT INTO valid_root
SELECT concept.concept_id FROM concept
WHERE NOT EXISTS (SELECT 1 FROM concept_parent WHERE concept_parent.concept_id=concept.concept_id)
  AND NOT EXISTS (SELECT 1 FROM master.concept_parent WHERE master.concept_parent.concept_id=concept.concept_id);
CREATE TEMP TABLE reachable(concept_id TEXT PRIMARY KEY) WITHOUT ROWID;
WITH RECURSIVE retained(concept_id) AS (
 SELECT concept_id FROM valid_root
 UNION
 SELECT edge.concept_id FROM retained JOIN concept_parent edge ON edge.parent_concept_id=retained.concept_id
)
INSERT INTO reachable SELECT concept_id FROM retained;
DELETE FROM concept_parent
WHERE concept_id NOT IN (SELECT concept_id FROM reachable)
   OR parent_concept_id NOT IN (SELECT concept_id FROM reachable);
DELETE FROM concept WHERE concept_id NOT IN (SELECT concept_id FROM reachable);

DROP TABLE reachable;
DROP TABLE valid_root;
DROP TABLE merge_member;
DROP TABLE merge_seed;
COMMIT;
DETACH DATABASE master;

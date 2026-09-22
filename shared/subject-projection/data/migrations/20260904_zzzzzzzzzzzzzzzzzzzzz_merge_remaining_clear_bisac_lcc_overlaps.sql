-- Merge the five remaining clear BISAC/LCC conceptual overlap groups.
-- The BISAC art concept is retained for Mixed Media; its crafts homonym stays separate.
-- The BISAC art concept is retained for Portraits; collective biography stays separate.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

CREATE TEMP TABLE bisac_lcc_merge(
  source_id TEXT PRIMARY KEY,
  target_id TEXT NOT NULL
) WITHOUT ROWID;

INSERT INTO bisac_lcc_merge VALUES
('bokheim:philosophy-lcc-full:d46e4765-d26f-5877-a933-17b4116b22f6','unified:445b25ed-0008-52e0-bc09-184098e55b88'),
('bokheim:lcc-detail:001662fb-31af-5274-8ebe-3aecd8a314d8','unified:6fa6c6a0-7690-5e71-bf9c-3f2acac93b9c'),
('bokheim:lcc-detail:0a0c6e57-02f2-5f44-86a0-a6be7643ce3b','unified:45108853-ebc3-5e32-9b93-2093cac4b11b'),
('bokheim:lcc-detail:efef3943-0093-553c-a4cd-d3cde7b03e32','unified:45108853-ebc3-5e32-9b93-2093cac4b11b'),
('bokheim:lcc-detail:056ff52f-6d6c-5046-9ca3-549c9e06baea','unified:0cc4ac6c-9b39-5036-8ee5-2f9d6cdb78b3'),
('bokheim:lcc-detail:f5292a8a-cbbf-54ab-84ec-1e7158bd576f','unified:c6d306ac-2ef7-5aba-8ea1-1684be91cc7f');

-- Preserve all external selectors, assigning fresh per-system ordinals.
WITH candidates AS (
 SELECT DISTINCT merge.target_id,selector.system_id,selector.selector
 FROM bisac_lcc_merge merge
 JOIN source_selector selector ON selector.concept_id=merge.source_id
 WHERE NOT EXISTS (
  SELECT 1 FROM source_selector existing
  WHERE existing.concept_id=merge.target_id
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

-- Preserve all distinct source wording.
WITH candidates AS (
 SELECT DISTINCT merge.target_id,label.label
 FROM bisac_lcc_merge merge JOIN source_label label ON label.concept_id=merge.source_id
 WHERE NOT EXISTS (
  SELECT 1 FROM source_label existing
  WHERE existing.concept_id=merge.target_id AND existing.label=label.label
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

-- Preserve every distinct browse parent.
WITH candidates AS (
 SELECT merge.target_id,edge.parent_concept_id,min(edge.display_order) display_order
 FROM bisac_lcc_merge merge JOIN concept_parent edge ON edge.concept_id=merge.source_id
 WHERE edge.parent_concept_id<>merge.target_id
   AND NOT EXISTS (
    SELECT 1 FROM concept_parent existing
    WHERE existing.concept_id=merge.target_id
      AND existing.parent_concept_id=edge.parent_concept_id
   )
 GROUP BY merge.target_id,edge.parent_concept_id
), numbered AS (
 SELECT candidates.*,
        row_number() OVER (PARTITION BY target_id ORDER BY display_order,parent_concept_id)-1 offset
 FROM candidates
)
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal,display_order)
SELECT target_id,parent_concept_id,
       COALESCE((SELECT max(existing.ordinal)+1 FROM concept_parent existing
                 WHERE existing.concept_id=numbered.target_id),0)+offset,
       display_order
FROM numbered;

-- Retain all children of a merged source concept.
WITH candidates AS (
 SELECT edge.concept_id,merge.target_id AS parent_concept_id,min(edge.display_order) display_order
 FROM bisac_lcc_merge merge JOIN concept_parent edge ON edge.parent_concept_id=merge.source_id
 WHERE edge.concept_id<>merge.target_id
   AND NOT EXISTS (
    SELECT 1 FROM concept_parent existing
    WHERE existing.concept_id=edge.concept_id
      AND existing.parent_concept_id=merge.target_id
   )
 GROUP BY edge.concept_id,merge.target_id
)
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal,display_order)
SELECT concept_id,parent_concept_id,
       COALESCE((SELECT max(existing.ordinal)+1 FROM concept_parent existing
                 WHERE existing.concept_id=candidates.concept_id),0),
       display_order
FROM candidates;

DELETE FROM concept_parent
WHERE concept_id IN (SELECT source_id FROM bisac_lcc_merge)
   OR parent_concept_id IN (SELECT source_id FROM bisac_lcc_merge);
DELETE FROM concept WHERE concept_id IN (SELECT source_id FROM bisac_lcc_merge);

DROP TABLE bisac_lcc_merge;
COMMIT;

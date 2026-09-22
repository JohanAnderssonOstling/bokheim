-- Merge high-confidence conceptual equivalents where one concept carries one
-- BISAC selector and the other carries one LCC selector.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

CREATE TEMP TABLE bisac_lcc_merge(
  source_id TEXT PRIMARY KEY,
  target_id TEXT NOT NULL
) WITHOUT ROWID;

-- Retain the BISAC concept and merge the equivalent LCC concept into it.
INSERT INTO bisac_lcc_merge VALUES
('bokheim:lcc-detail:779a394e-b0a8-5f9f-bf6e-a460530e3ae6','unified:93084829-108d-583e-a2b1-21c203e19248'),
('bokheim:lcc-detail:e07e5237-58ad-5fb7-aa6f-e8fa6ac85cdc','unified:4fab1a80-c078-57bc-a5aa-461e92a5ace8'),
('bokheim:lcc-detail:aed8a5e6-e241-5071-8ec7-a2f1b1d3aa7a','unified:95ed9d12-b3f1-5e57-a4c5-5adb14687a13'),
('bokheim:lcc-detail:7bf66ded-9f2e-5a76-8946-4a31d07eb167','unified:91db6c88-961d-5391-ac81-6306f93430b1'),
('bokheim:lcc-detail:cf5af304-c3b0-5268-bfd5-d7842ed055dc','unified:2364bc34-ea73-5cf2-a82e-b68b9db0c83c'),
('bokheim:lcc-detail:4f5d9dc1-b4e2-5c3e-ad93-2a194cc3a18d','unified:c31d7d12-f1f5-5450-b33d-556c2f40baaf'),
('bokheim:lcc-detail:cb2ff26a-cacc-5738-874e-0fe9e200d92d','unified:21919ffd-0a83-54f1-a9b4-146a252fcb84'),
('bokheim:lcc-detail:15e602c8-7a0e-5a6d-801f-4ab0741200cd','unified:b74a2387-8e20-576b-acf4-fe30436ce925'),
('bokheim:lcc-detail:1afa66f3-f709-5f3b-89e2-ec005e6fbdf6','unified:e127efd3-7b97-5afd-9890-6ca7ceb54cdc'),
('bokheim:lcc-detail:f0c88603-a4bd-5909-bd4e-e8df7df1b973','unified:5deceb91-cdbe-5579-9dd1-0f11d544782e'),
('bokheim:lcc-detail:6c7ffb69-beab-5e47-89d8-c652788e8eff','unified:5b347b71-f4c8-544e-b09e-308e6be61641'),
('bokheim:lcc-detail:81fcafd6-4a5d-55dd-ae17-0e03c925bea9','unified:59576379-8743-5e69-a0aa-bf357b5e1ac4'),
('unified:668ee328-3ec6-5b64-a86b-025e4ef79e4c','unified:5bcc639e-2b5b-54c6-9506-e9e5a9f7a29b'),
('bokheim:lcc-detail:4038cb44-d0a6-5e38-899d-c16208ea11f9','unified:cadc4cbe-eebd-572e-80f9-5573eb566bd0'),
('bokheim:lcc-detail:16b0f494-fb23-55d4-92ce-76e538f2e6aa','unified:78e7d9d7-06a9-5666-ae0f-1c39bb606ec0'),
('bokheim:lcc-detail:fc453769-ed2b-58b7-b558-58658a71eeb0','unified:11e1f876-c86b-5a15-9741-f08aaac76e3e'),
('unified:7c5a2840-77b1-5f5f-be01-a3af04d70112','unified:7c084914-e101-5ce3-a401-c7a5aed07246'),
('bokheim:lcc-detail:296a8cf3-a351-5427-b82c-0f6c97303076','unified:e71c1e18-3630-5803-9ca3-0ec75dfd52bf');

-- Preserve all external selectors.
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

-- Preserve source wording.
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

-- Retain every distinct browse parent.
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

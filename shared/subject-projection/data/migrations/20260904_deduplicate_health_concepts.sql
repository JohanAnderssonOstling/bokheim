-- Merge equivalent health concepts without forcing broad composite selectors
-- onto narrower retained subjects.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

CREATE TEMP TABLE health_merge(
  source_id TEXT PRIMARY KEY,
  target_id TEXT NOT NULL
) WITHOUT ROWID;

INSERT INTO health_merge VALUES
 -- Pharmacology and therapeutics: retain the detailed LCC concept.
 ('unified:b3594207-7bfc-5508-9d64-d21d37f14f47','unified:c41a2c2d-0a97-553f-a72d-66028c08f8ca'),
 ('unified:bc41b1c6-04ab-502b-8ab5-a81d34855245','unified:c41a2c2d-0a97-553f-a72d-66028c08f8ca'),
 -- Pharmacy: retain the concise BISAC concept and its direct position.
 ('unified:55a9b081-356e-53a3-91a4-5d4416f4cdc1','unified:48ae8643-7e51-54d3-beb2-fb13a78dba84'),
 -- Exact concepts duplicated between source systems or curated contexts.
 ('unified:da27de75-bcdf-51dc-99e8-58592d884a4b','unified:6efba34c-d617-5902-9e1a-800f4d541cfc'),
 ('unified:2e724df6-3e63-539c-b75e-841e23bb3f8a','unified:f00a6ff7-f7a7-55f5-ab51-be4d82de2713'),
 ('unified:d1d554aa-56ae-5014-b92a-896d5db5b8ec','unified:dfa758aa-a392-591b-bb7e-f693e6fff258'),
 ('unified:227097ab-5e8e-51f7-9866-6fcecf60f6ed','unified:1d647fb2-7aa9-5c75-b121-ca7a1b56fe63'),
 ('unified:229bc4b7-87b6-566b-af9e-45500e801876','unified:25f2774a-e514-551a-81ab-94dc4455dce2'),
 ('unified:4ab9d3de-4710-5353-9809-78f3e5a4b87e','unified:aba3d656-b4aa-568c-916c-8aab62297180'),
 ('unified:1cb50b28-cbf7-5917-93e9-a09f372c8d2c','unified:aba3d656-b4aa-568c-916c-8aab62297180'),
 ('unified:7e7633be-f609-5b86-9f0e-5d5ae7abf3e0','unified:b5e344c1-d648-5d4f-9312-ccb39b3a99c8'),
 ('bokheim:lcc-detail:daf6927c-81ba-5ca4-a694-139019afb309','unified:8e9e622d-b068-5c41-ab0a-34d27acfed2a'),
 ('bokheim:lcc-detail:7fcef552-d898-55f6-8109-8487b5f6c3b2','unified:e6218238-925b-5624-ad2c-f854caf31d1d'),
 ('bokheim:lcc-detail:1b111108-41d8-5ee8-baf0-f0edc288a9ef','unified:5c900858-6d3c-5ac2-b756-e1574b19490d'),
 ('bokheim:lcc-detail:a63b7d32-47fe-588d-813e-0a6aebc85bae','unified:690a4f65-fef0-5357-9e79-ead892c4bcf8'),
 ('bokheim:lcc-detail:b98c2e23-53af-5f2e-84b4-06dd284f7ded','unified:cc1c53d4-6866-5c80-91cd-64415c80965c'),
 ('bokheim:lcc-detail:88b15e88-dbfb-58f7-9189-e865cc87f1a5','unified:a0e3f2f9-9f4d-569b-b609-ca240506c354'),
 ('unified:9ad38402-eaf5-5940-9cb9-1ff5bcf6d5f8','unified:a0e3f2f9-9f4d-569b-b609-ca240506c354');

-- Preserve all external selectors on the retained concept.
WITH candidates AS (
 SELECT DISTINCT merge.target_id,selector.system_id,selector.selector
 FROM health_merge merge
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
 FROM health_merge merge JOIN source_label label ON label.concept_id=merge.source_id
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

-- Retain every distinct parent. This deliberately creates one reusable concept
-- with multiple browse locations when the same subject occurs in several areas.
WITH candidates AS (
 SELECT merge.target_id,edge.parent_concept_id,min(edge.display_order) display_order
 FROM health_merge merge JOIN concept_parent edge ON edge.concept_id=merge.source_id
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
 FROM health_merge merge JOIN concept_parent edge ON edge.parent_concept_id=merge.source_id
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

UPDATE concept SET preferred_label='Pharmacology & Therapeutics'
WHERE concept_id='unified:c41a2c2d-0a97-553f-a72d-66028c08f8ca';

DELETE FROM concept_parent
WHERE concept_id IN (SELECT source_id FROM health_merge)
   OR parent_concept_id IN (SELECT source_id FROM health_merge);
DELETE FROM concept WHERE concept_id IN (SELECT source_id FROM health_merge);

DROP TABLE health_merge;
COMMIT;

PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;

-- Merge clear LCC equivalents into the curated political subjects. The
-- political concepts remain canonical and retain their existing browse parent.
CREATE TEMP TABLE politics_lcc_merge(
  source_id TEXT PRIMARY KEY,
  target_id TEXT NOT NULL
) WITHOUT ROWID;

INSERT INTO politics_lcc_merge VALUES
('unified:c68b0161-7978-53f0-9c12-c23879ea6b40',
 'unified:93949cc9-94a2-5ebc-9fb4-d366be2cc1a9'),
('bokheim:lcc-detail:7fd2f4a5-faa0-5452-96ec-05b42675dff9',
 'unified:17ec5c74-ee08-5415-b1e0-6f7814da7a3a'),
('unified:fa5ca91b-aa32-5d01-9ef9-e5153aed98ab',
 'unified:b1f5439f-4b60-55d5-94c5-353a31a4d6a4'),
('unified:bf2818d9-49c8-5224-b38e-e2dcfc1e55de',
 'unified:b9591cf1-8997-548d-bec3-c8ead377e05e');

WITH candidates AS (
 SELECT DISTINCT merge.target_id,selector.system_id,selector.selector
 FROM politics_lcc_merge merge
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

-- Retain children, notably Book Burning beneath Censorship.
WITH candidates AS (
 SELECT edge.concept_id,merge.target_id AS parent_concept_id,
        min(edge.display_order) AS display_order
 FROM politics_lcc_merge merge
 JOIN concept_parent edge ON edge.parent_concept_id=merge.source_id
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
WHERE concept_id IN (SELECT source_id FROM politics_lcc_merge)
   OR parent_concept_id IN (SELECT source_id FROM politics_lcc_merge);
DELETE FROM concept WHERE concept_id IN (SELECT source_id FROM politics_lcc_merge);

UPDATE concept SET preferred_label='Freedom of the Press & Censorship'
WHERE concept_id='unified:93949cc9-94a2-5ebc-9fb4-d366be2cc1a9';
UPDATE concept SET preferred_label='Public Opinion & Polling'
WHERE concept_id='unified:17ec5c74-ee08-5415-b1e0-6f7814da7a3a';
UPDATE concept SET preferred_label='Law Enforcement & Police'
WHERE concept_id='unified:b1f5439f-4b60-55d5-94c5-353a31a4d6a4';
UPDATE concept SET preferred_label='Peace & Conflict Resolution'
WHERE concept_id='unified:b9591cf1-8997-548d-bec3-c8ead377e05e';

-- Split international crimes out of the over-combined Law of nations leaf.
UPDATE source_selector
SET concept_id='unified:22713bad-bc3b-5215-ae6f-bbf08c28d5c8',
    ordinal=CASE selector
      WHEN 'KZ7145..KZ7177@5' THEN 0
      WHEN 'KZ7180..KZ7188@5' THEN 1
    END
WHERE concept_id='unified:daf6b15e-d5bc-5e9b-887f-714ede8b99fd'
  AND system_id='lcc'
  AND selector IN ('KZ7145..KZ7177@5','KZ7180..KZ7188@5');

DROP TABLE politics_lcc_merge;
COMMIT;

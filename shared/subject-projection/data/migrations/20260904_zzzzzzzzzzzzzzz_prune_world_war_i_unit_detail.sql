-- Prune unit and filing detail beneath World War I while retaining military
-- operations, theaters, national operations, and named campaigns.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

CREATE TEMP TABLE ww1_fallback(
    source_id TEXT PRIMARY KEY,
    target_id TEXT NOT NULL
) WITHOUT ROWID;

INSERT INTO ww1_fallback VALUES
    -- Redundant US operations wrapper and its camp filing detail.
    ('bokheim:history-lcc-full:d42467d2-043a-5e0e-bfb6-6ec766a94282',
     'bokheim:history-lcc-full:89079aca-9614-5fdd-8860-e2f2122cbc2b'),
    ('bokheim:history-lcc-full:b4ed0d19-4bf8-59c6-9f89-bf16e0a9cf9c',
     'bokheim:history-lcc-full:89079aca-9614-5fdd-8860-e2f2122cbc2b'),

    -- Individual formations and nationality filing subdivisions.
    ('bokheim:history-lcc-full:952603d4-aa66-5d7e-bf9a-4150deec1763',
     'bokheim:history-lcc-full:24bd6d4d-6426-5096-be87-7d7b67d6d601'),
    ('bokheim:history-lcc-full:26659806-ee98-5aca-997f-4d3b05293c19',
     'bokheim:history-lcc-full:3717ef3e-2753-5de0-9fc9-ec5e84fb9a59'),
    ('bokheim:history-lcc-full:78cc6183-a357-59ed-aaa9-9dc9efd6670f',
     'bokheim:history-lcc-full:3717ef3e-2753-5de0-9fc9-ec5e84fb9a59'),
    ('bokheim:history-lcc-full:2427f8c3-d3cc-5be3-aae8-9b760fe808ac',
     'bokheim:history-lcc-full:d60c6581-970e-5f78-ab27-7e2849e47e44'),
    ('bokheim:history-lcc-full:49509e02-4e3f-5058-91e8-407b26098fa9',
     'bokheim:history-lcc-full:d60c6581-970e-5f78-ab27-7e2849e47e44'),
    ('bokheim:history-lcc-full:70e66853-acf1-521b-95e0-b1278025d6a2',
     'bokheim:history-lcc-full:d60c6581-970e-5f78-ab27-7e2849e47e44'),
    ('bokheim:history-lcc-full:73997309-b466-5200-9e0b-b2fa74d22a22',
     'bokheim:history-lcc-full:d60c6581-970e-5f78-ab27-7e2849e47e44'),
    ('bokheim:history-lcc-full:d2543122-f48c-51cc-b488-404a7a38d1e5',
     'bokheim:history-lcc-full:d60c6581-970e-5f78-ab27-7e2849e47e44'),
    ('bokheim:history-lcc-full:d6b7c9d7-80ec-596c-a425-1c8c33c1aa7e',
     'bokheim:history-lcc-full:d60c6581-970e-5f78-ab27-7e2849e47e44'),
    ('bokheim:history-lcc-full:3825ab33-a280-57cf-b1c1-c4f70a9f8b7f',
     'bokheim:history-lcc-full:807f6cd1-686a-5710-89ca-a9404c571a4d'),
    ('bokheim:history-lcc-full:a90e6d44-b82a-5c27-b790-d515b4e010d2',
     'bokheim:history-lcc-full:807f6cd1-686a-5710-89ca-a9404c571a4d'),
    ('bokheim:history-lcc-full:edc29bce-c382-5f70-a333-baa7d3a5cc45',
     'bokheim:history-lcc-full:807f6cd1-686a-5710-89ca-a9404c571a4d'),
    ('bokheim:history-lcc-full:0a83ba7c-beea-5120-b3ed-f93846c77e3a',
     'bokheim:history-lcc-full:eab32e5b-980a-53bc-aa7f-c1cb4e946eae'),
    ('bokheim:history-lcc-full:d2f9f038-7229-5072-8f7b-d40b423edc4d',
     'bokheim:history-lcc-full:eab32e5b-980a-53bc-aa7f-c1cb4e946eae'),
    ('bokheim:history-lcc-full:649bc176-e14d-55b0-8ab0-32890ee5b24c',
     'bokheim:history-lcc-full:f112badb-928d-5d32-a483-b386fe4da04e');

-- Preserve the African theater without the intervening German filing layer.
INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal,display_order)
SELECT 'bokheim:history-lcc-full:75d26a6c-1e1e-5e72-98bb-53f2e54ab70b',
       'bokheim:history-lcc-full:f112badb-928d-5d32-a483-b386fe4da04e',
       COALESCE((SELECT max(ordinal)+1 FROM concept_parent
                 WHERE concept_id='bokheim:history-lcc-full:75d26a6c-1e1e-5e72-98bb-53f2e54ab70b'),0),
       10;

-- Move each removed node's selector to its nearest meaningful retained node.
WITH moved AS (
    SELECT f.target_id,s.system_id,s.selector,
           row_number() OVER (
               PARTITION BY f.target_id,s.system_id ORDER BY f.source_id,s.ordinal
           ) AS offset
    FROM ww1_fallback f
    JOIN source_selector s ON s.concept_id=f.source_id
)
INSERT OR IGNORE INTO source_selector(concept_id,system_id,selector,ordinal)
SELECT moved.target_id,moved.system_id,moved.selector,
       COALESCE((SELECT max(existing.ordinal)+1
                 FROM source_selector existing
                 WHERE existing.concept_id=moved.target_id
                   AND existing.system_id=moved.system_id),0)
       + moved.offset - 1
FROM moved;

DELETE FROM concept_parent
WHERE concept_id IN (SELECT source_id FROM ww1_fallback)
   OR parent_concept_id IN (SELECT source_id FROM ww1_fallback);

DELETE FROM concept
WHERE concept_id IN (SELECT source_id FROM ww1_fallback)
  AND NOT EXISTS (
      SELECT 1 FROM concept_parent remaining
      WHERE remaining.concept_id=concept.concept_id
         OR remaining.parent_concept_id=concept.concept_id
  );

DROP TABLE ww1_fallback;
COMMIT;

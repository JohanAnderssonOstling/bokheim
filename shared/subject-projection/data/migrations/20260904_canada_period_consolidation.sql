-- Reduce curated Canada to a flat, non-overlapping historical chronology.
-- All richer topical and geographic structure remains in the master taxonomy.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

CREATE TEMP TABLE canada_period(
    concept_id TEXT PRIMARY KEY,
    preferred_label TEXT NOT NULL,
    display_order INTEGER NOT NULL
) WITHOUT ROWID;
INSERT INTO canada_period VALUES
    ('bokheim:history-lcc-full:473652e0-019f-5edb-b2df-847b0f0b37f7', '1603-1763. New France', 10),
    ('bokheim:history-lcc-full:5e1228f9-d9ce-5edb-b389-0b3ca431b100', '1763-1867', 20),
    ('bokheim:history-lcc-full:9430077d-d7fc-5ed6-9138-78ad8456f763', '1867-1914. Dominion of Canada', 30),
    ('bokheim:history-lcc-full:72fc7fea-8fa3-5159-b65f-834f3d46a361', '1914-1945', 40),
    ('bokheim:history-lcc-full:2131051f-28be-55ff-9695-3d34e118a4b0', '1945-', 50);

-- The Seven Years' War range is wholly contained in the New France period.
INSERT INTO source_selector(concept_id, system_id, selector, ordinal)
SELECT 'bokheim:history-lcc-full:473652e0-019f-5edb-b2df-847b0f0b37f7',
       source.system_id,
       source.selector,
       COALESCE((SELECT max(ordinal) FROM source_selector
                 WHERE concept_id='bokheim:history-lcc-full:473652e0-019f-5edb-b2df-847b0f0b37f7'
                   AND system_id=source.system_id), -1)
       + row_number() OVER (PARTITION BY source.system_id ORDER BY source.ordinal)
FROM source_selector source
WHERE source.concept_id='bokheim:history-lcc-full:b6f97ed0-0a9f-525c-a902-d4138bdd9f4a'
  AND NOT EXISTS (
      SELECT 1 FROM source_selector existing
      WHERE existing.concept_id='bokheim:history-lcc-full:473652e0-019f-5edb-b2df-847b0f0b37f7'
        AND existing.system_id=source.system_id
        AND existing.selector=source.selector
  );

CREATE TEMP TABLE canada_detail(concept_id TEXT PRIMARY KEY) WITHOUT ROWID;
WITH RECURSIVE subtree(concept_id) AS (
    VALUES ('unified:704e7f13-642a-5dd0-a2f0-56c3dc29161e')
    UNION
    SELECT child.concept_id
    FROM concept_parent child
    JOIN subtree parent ON parent.concept_id=child.parent_concept_id
)
INSERT INTO canada_detail
SELECT concept_id FROM subtree
WHERE concept_id<>'unified:704e7f13-642a-5dd0-a2f0-56c3dc29161e'
  AND concept_id NOT IN (SELECT concept_id FROM canada_period);

-- Detach retained nested periods before deleting their former wrappers.
DELETE FROM concept_parent
WHERE concept_id IN (SELECT concept_id FROM canada_period)
  AND parent_concept_id IN (
      SELECT concept_id FROM canada_detail
      UNION SELECT 'unified:704e7f13-642a-5dd0-a2f0-56c3dc29161e'
  );

INSERT INTO concept_parent(concept_id, parent_concept_id, ordinal, display_order)
SELECT period.concept_id,
       'unified:704e7f13-642a-5dd0-a2f0-56c3dc29161e',
       COALESCE((SELECT max(ordinal) + 1 FROM concept_parent WHERE concept_id=period.concept_id), 0),
       period.display_order
FROM canada_period period;

UPDATE concept
SET preferred_label=(SELECT preferred_label FROM canada_period WHERE canada_period.concept_id=concept.concept_id)
WHERE concept_id IN (SELECT concept_id FROM canada_period);

DELETE FROM concept_parent
WHERE concept_id IN (SELECT concept_id FROM canada_detail)
   OR parent_concept_id IN (SELECT concept_id FROM canada_detail);
DELETE FROM concept WHERE concept_id IN (SELECT concept_id FROM canada_detail);

DROP TABLE canada_detail;
DROP TABLE canada_period;
COMMIT;

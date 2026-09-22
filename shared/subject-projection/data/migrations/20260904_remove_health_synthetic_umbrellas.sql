-- Remove selector-free curated umbrellas directly beneath Health & Medicine.
-- Their existing children become direct children; external selectors remain on
-- the source-backed concepts to which they already belong.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

CREATE TEMP TABLE health_wrapper(
  concept_id TEXT PRIMARY KEY,
  display_order INTEGER NOT NULL
) WITHOUT ROWID;

INSERT INTO health_wrapper(concept_id,display_order)
SELECT child.concept_id,edge.display_order
FROM concept_parent edge
JOIN concept child ON child.concept_id=edge.concept_id
WHERE edge.parent_concept_id='unified:353c91d5-53e5-56a8-9cd6-544a69566d18'
  AND NOT EXISTS (
    SELECT 1 FROM source_selector selector
    WHERE selector.concept_id=child.concept_id
  )
  AND EXISTS (
    SELECT 1 FROM concept_parent descendant
    WHERE descendant.parent_concept_id=child.concept_id
  );

CREATE TEMP TABLE promoted(
  concept_id TEXT PRIMARY KEY,
  wrapper_order INTEGER NOT NULL,
  child_order INTEGER NOT NULL
) WITHOUT ROWID;

INSERT INTO promoted(concept_id,wrapper_order,child_order)
SELECT edge.concept_id,min(wrapper.display_order),min(edge.display_order)
FROM health_wrapper wrapper
JOIN concept_parent edge ON edge.parent_concept_id=wrapper.concept_id
WHERE NOT EXISTS (
  SELECT 1 FROM concept_parent existing
  WHERE existing.concept_id=edge.concept_id
    AND existing.parent_concept_id='unified:353c91d5-53e5-56a8-9cd6-544a69566d18'
)
GROUP BY edge.concept_id;

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal,display_order)
SELECT promoted.concept_id,
       'unified:353c91d5-53e5-56a8-9cd6-544a69566d18',
       COALESCE((SELECT max(existing.ordinal)+1
                 FROM concept_parent existing
                 WHERE existing.concept_id=promoted.concept_id),0),
       promoted.wrapper_order*1000+promoted.child_order
FROM promoted;

DELETE FROM concept_parent
WHERE parent_concept_id IN (SELECT concept_id FROM health_wrapper);
DELETE FROM concept_parent
WHERE concept_id IN (SELECT concept_id FROM health_wrapper)
  AND parent_concept_id='unified:353c91d5-53e5-56a8-9cd6-544a69566d18';

-- These are intentionally selector-free and have now lost their only curated
-- parent and all outgoing edges, so their concepts carry no remaining meaning.
DELETE FROM concept
WHERE concept_id IN (SELECT concept_id FROM health_wrapper)
  AND NOT EXISTS (SELECT 1 FROM concept_parent edge WHERE edge.concept_id=concept.concept_id)
  AND NOT EXISTS (SELECT 1 FROM concept_parent edge WHERE edge.parent_concept_id=concept.concept_id)
  AND NOT EXISTS (SELECT 1 FROM source_selector selector WHERE selector.concept_id=concept.concept_id);

-- Normalize the expanded root ordering while preserving wrapper/child order.
WITH ordered AS (
  SELECT concept_id,
         row_number() OVER (ORDER BY display_order,concept_id)-1 AS new_order
  FROM concept_parent
  WHERE parent_concept_id='unified:353c91d5-53e5-56a8-9cd6-544a69566d18'
)
UPDATE concept_parent
SET display_order=(SELECT new_order FROM ordered
                   WHERE ordered.concept_id=concept_parent.concept_id)
WHERE parent_concept_id='unified:353c91d5-53e5-56a8-9cd6-544a69566d18';

DROP TABLE promoted;
DROP TABLE health_wrapper;
COMMIT;

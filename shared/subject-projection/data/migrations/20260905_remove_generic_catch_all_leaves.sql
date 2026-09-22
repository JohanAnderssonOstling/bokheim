-- Remove generic LCC filing buckets and retain their coverage on the
-- immediate meaningful subject parent.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

CREATE TEMP TABLE generic_leaf(
    concept_id TEXT PRIMARY KEY,
    parent_id TEXT NOT NULL
) WITHOUT ROWID;

INSERT INTO generic_leaf
SELECT child.concept_id,edge.parent_concept_id
FROM concept child
JOIN concept_parent edge ON edge.concept_id=child.concept_id
WHERE lower(child.preferred_label) IN (
    'other',
    'by form',
    'special topics',
    'topics not elsewhere provided for'
);

-- Promote meaningful children of a catch-all wrapper before removing it.
UPDATE concept_parent AS child_edge
SET parent_concept_id=(
    SELECT leaf.parent_id FROM generic_leaf leaf
    WHERE leaf.concept_id=child_edge.parent_concept_id
)
WHERE child_edge.parent_concept_id IN (SELECT concept_id FROM generic_leaf);

WITH moved AS (
    SELECT leaf.parent_id,s.system_id,s.selector,
           row_number() OVER (
               PARTITION BY leaf.parent_id,s.system_id
               ORDER BY leaf.concept_id,s.ordinal
           ) AS offset
    FROM generic_leaf leaf
    JOIN source_selector s ON s.concept_id=leaf.concept_id
)
INSERT OR IGNORE INTO source_selector(concept_id,system_id,selector,ordinal)
SELECT moved.parent_id,moved.system_id,moved.selector,
       COALESCE((SELECT max(existing.ordinal)+1
                 FROM source_selector existing
                 WHERE existing.concept_id=moved.parent_id
                   AND existing.system_id=moved.system_id),0)
       + moved.offset - 1
FROM moved;

DELETE FROM concept_parent
WHERE concept_id IN (SELECT concept_id FROM generic_leaf);
DELETE FROM concept
WHERE concept_id IN (SELECT concept_id FROM generic_leaf);

DROP TABLE generic_leaf;

-- Recovery for an interrupted earlier application that detached the wrappers
-- after copying their selectors but before deleting a wrapper with children.
CREATE TEMP TABLE orphan_generic(concept_id TEXT PRIMARY KEY,target_id TEXT NOT NULL) WITHOUT ROWID;
INSERT INTO orphan_generic
SELECT generic.concept_id,min(owner.concept_id)
FROM concept generic
JOIN source_selector own ON own.concept_id=generic.concept_id
JOIN source_selector owner
  ON owner.system_id=own.system_id
 AND owner.selector=own.selector
 AND owner.concept_id<>generic.concept_id
WHERE lower(generic.preferred_label) IN (
    'other','by form','special topics','topics not elsewhere provided for'
)
  AND NOT EXISTS (SELECT 1 FROM concept_parent edge WHERE edge.concept_id=generic.concept_id)
GROUP BY generic.concept_id;

UPDATE concept_parent AS child_edge
SET parent_concept_id=(
    SELECT orphan.target_id FROM orphan_generic orphan
    WHERE orphan.concept_id=child_edge.parent_concept_id
)
WHERE child_edge.parent_concept_id IN (SELECT concept_id FROM orphan_generic);

DELETE FROM concept
WHERE concept_id IN (SELECT concept_id FROM orphan_generic);
DROP TABLE orphan_generic;
COMMIT;

-- Make the Franco-Prussian War a leaf in the curated taxonomy. Its complete
-- topical breakdown remains available in the master taxonomy.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

CREATE TEMP TABLE franco_prussian_children(concept_id TEXT PRIMARY KEY) WITHOUT ROWID;
WITH RECURSIVE subtree(concept_id) AS (
    SELECT child.concept_id
    FROM concept_parent child
    WHERE child.parent_concept_id='bokheim:lcc-detail:2ed7ae64-1c47-5f80-b0b3-3d944240f22c'
    UNION
    SELECT child.concept_id
    FROM subtree parent
    JOIN concept_parent child ON child.parent_concept_id=parent.concept_id
)
INSERT INTO franco_prussian_children SELECT concept_id FROM subtree;

-- Preserve any concept also attached outside this subtree, plus descendants
-- that depend upon it. The current branch has no such shared concepts.
CREATE TEMP TABLE franco_prussian_shared(concept_id TEXT PRIMARY KEY) WITHOUT ROWID;
WITH RECURSIVE shared(concept_id) AS (
    SELECT subtree.concept_id
    FROM franco_prussian_children subtree
    JOIN concept_parent edge ON edge.concept_id=subtree.concept_id
    WHERE edge.parent_concept_id<>'bokheim:lcc-detail:2ed7ae64-1c47-5f80-b0b3-3d944240f22c'
      AND edge.parent_concept_id NOT IN (SELECT concept_id FROM franco_prussian_children)
    UNION
    SELECT child.concept_id
    FROM shared parent
    JOIN concept_parent child ON child.parent_concept_id=parent.concept_id
    WHERE child.concept_id IN (SELECT concept_id FROM franco_prussian_children)
)
INSERT INTO franco_prussian_shared SELECT concept_id FROM shared;

CREATE TEMP TABLE franco_prussian_remove(concept_id TEXT PRIMARY KEY) WITHOUT ROWID;
INSERT INTO franco_prussian_remove
SELECT concept_id FROM franco_prussian_children
WHERE concept_id NOT IN (SELECT concept_id FROM franco_prussian_shared);

DELETE FROM concept_parent
WHERE concept_id IN (SELECT concept_id FROM franco_prussian_remove)
   OR parent_concept_id IN (SELECT concept_id FROM franco_prussian_remove);
DELETE FROM concept WHERE concept_id IN (SELECT concept_id FROM franco_prussian_remove);

DROP TABLE franco_prussian_remove;
DROP TABLE franco_prussian_shared;
DROP TABLE franco_prussian_children;
COMMIT;

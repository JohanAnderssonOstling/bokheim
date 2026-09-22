-- Remove the campaigns-and-battles detail branch from the curated taxonomy.
-- The complete classification remains available in the master taxonomy.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

CREATE TEMP TABLE campaign_tree(concept_id TEXT PRIMARY KEY) WITHOUT ROWID;
WITH RECURSIVE tree(concept_id) AS (
    VALUES ('bokheim:history-lcc:6f78f450-0215-510c-b471-04a73b26550b')
    UNION
    SELECT child.concept_id
    FROM concept_parent child
    JOIN tree parent ON parent.concept_id=child.parent_concept_id
)
INSERT INTO campaign_tree SELECT concept_id FROM tree;

-- Preserve any descendant shared with a branch outside this subtree, together
-- with its descendants. The current curated graph has none, but this keeps the
-- migration safe if it is replayed against a compatible expanded graph.
CREATE TEMP TABLE campaign_shared(concept_id TEXT PRIMARY KEY) WITHOUT ROWID;
WITH RECURSIVE shared(concept_id) AS (
    SELECT tree.concept_id
    FROM campaign_tree tree
    JOIN concept_parent edge ON edge.concept_id=tree.concept_id
    WHERE tree.concept_id<>'bokheim:history-lcc:6f78f450-0215-510c-b471-04a73b26550b'
      AND edge.parent_concept_id NOT IN (SELECT concept_id FROM campaign_tree)
    UNION
    SELECT child.concept_id
    FROM shared parent
    JOIN concept_parent child ON child.parent_concept_id=parent.concept_id
    WHERE child.concept_id IN (SELECT concept_id FROM campaign_tree)
)
INSERT INTO campaign_shared SELECT concept_id FROM shared;

CREATE TEMP TABLE campaign_remove(concept_id TEXT PRIMARY KEY) WITHOUT ROWID;
INSERT INTO campaign_remove
SELECT concept_id FROM campaign_tree
WHERE concept_id NOT IN (SELECT concept_id FROM campaign_shared);

DELETE FROM concept_parent
WHERE concept_id IN (SELECT concept_id FROM campaign_remove)
   OR parent_concept_id IN (SELECT concept_id FROM campaign_remove);
DELETE FROM concept WHERE concept_id IN (SELECT concept_id FROM campaign_remove);

DROP TABLE campaign_remove;
DROP TABLE campaign_shared;
DROP TABLE campaign_tree;
COMMIT;

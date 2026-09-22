-- Remove Law by Jurisdiction from the curated hierarchy while retaining any
-- concepts also used by other branches. The master taxonomy is unchanged.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

CREATE TEMP TABLE jurisdiction_tree(concept_id TEXT PRIMARY KEY) WITHOUT ROWID;
WITH RECURSIVE tree(concept_id) AS (
    VALUES ('bokheim:subject:law:jurisdiction')
    UNION
    SELECT child.concept_id FROM concept_parent child
    JOIN tree parent ON parent.concept_id=child.parent_concept_id
)
INSERT INTO jurisdiction_tree SELECT concept_id FROM tree;

CREATE TEMP TABLE jurisdiction_shared(concept_id TEXT PRIMARY KEY) WITHOUT ROWID;
WITH RECURSIVE preserved(concept_id) AS (
    SELECT tree.concept_id
    FROM jurisdiction_tree tree
    JOIN concept_parent edge ON edge.concept_id=tree.concept_id
    WHERE tree.concept_id<>'bokheim:subject:law:jurisdiction'
      AND edge.parent_concept_id NOT IN (SELECT concept_id FROM jurisdiction_tree)
    UNION
    SELECT child.concept_id FROM concept_parent child
    JOIN preserved parent ON parent.concept_id=child.parent_concept_id
    WHERE child.concept_id IN (SELECT concept_id FROM jurisdiction_tree)
)
INSERT INTO jurisdiction_shared SELECT concept_id FROM preserved;

CREATE TEMP TABLE jurisdiction_remove(concept_id TEXT PRIMARY KEY) WITHOUT ROWID;
INSERT INTO jurisdiction_remove
SELECT concept_id FROM jurisdiction_tree
WHERE concept_id NOT IN (SELECT concept_id FROM jurisdiction_shared);

DELETE FROM concept_parent
WHERE concept_id IN (SELECT concept_id FROM jurisdiction_remove)
   OR parent_concept_id IN (SELECT concept_id FROM jurisdiction_remove);
DELETE FROM concept WHERE concept_id IN (SELECT concept_id FROM jurisdiction_remove);

DROP TABLE jurisdiction_remove;
DROP TABLE jurisdiction_shared;
DROP TABLE jurisdiction_tree;
COMMIT;

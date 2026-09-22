-- Enforce the Christianity exception as a strict maximum depth of three below
-- Religion. Nodes at Religion depth three are leaves.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

CREATE TEMP TABLE religion_tree(concept_id TEXT PRIMARY KEY,depth INTEGER NOT NULL) WITHOUT ROWID;
WITH RECURSIVE tree(concept_id,depth) AS (
 VALUES ('unified:a10eef28-4e52-5753-90f5-7d766aa96a7d',0)
 UNION ALL
 SELECT child.concept_id,tree.depth+1
 FROM tree JOIN concept_parent child ON child.parent_concept_id=tree.concept_id
)
INSERT INTO religion_tree SELECT concept_id,min(depth) FROM tree GROUP BY concept_id;

CREATE TEMP TABLE religion_deep(concept_id TEXT PRIMARY KEY) WITHOUT ROWID;
WITH RECURSIVE tree(concept_id,depth) AS (
 VALUES ('unified:a10eef28-4e52-5753-90f5-7d766aa96a7d',0)
 UNION ALL
 SELECT child.concept_id,tree.depth+1
 FROM tree JOIN concept_parent child ON child.parent_concept_id=tree.concept_id
)
INSERT INTO religion_deep SELECT DISTINCT concept_id FROM tree WHERE depth>=4;

CREATE TEMP TABLE religion_shared(concept_id TEXT PRIMARY KEY) WITHOUT ROWID;
WITH RECURSIVE preserved(concept_id) AS (
 SELECT deep.concept_id FROM religion_deep deep
 JOIN concept_parent edge ON edge.concept_id=deep.concept_id
 WHERE edge.parent_concept_id NOT IN (SELECT concept_id FROM religion_tree)
 UNION
 SELECT child.concept_id FROM preserved parent
 JOIN concept_parent child ON child.parent_concept_id=parent.concept_id
 WHERE child.concept_id IN (SELECT concept_id FROM religion_deep)
)
INSERT INTO religion_shared SELECT concept_id FROM preserved;

CREATE TEMP TABLE religion_remove(concept_id TEXT PRIMARY KEY) WITHOUT ROWID;
INSERT INTO religion_remove SELECT concept_id FROM religion_deep
WHERE concept_id NOT IN (SELECT concept_id FROM religion_shared);

DELETE FROM concept_parent
WHERE parent_concept_id IN (
 SELECT concept_id FROM religion_tree WHERE depth=3
);
DELETE FROM concept_parent
WHERE concept_id IN (SELECT concept_id FROM religion_remove)
   OR parent_concept_id IN (SELECT concept_id FROM religion_remove);
DELETE FROM concept WHERE concept_id IN (SELECT concept_id FROM religion_remove);

DROP TABLE religion_remove;
DROP TABLE religion_shared;
DROP TABLE religion_deep;
DROP TABLE religion_tree;
COMMIT;

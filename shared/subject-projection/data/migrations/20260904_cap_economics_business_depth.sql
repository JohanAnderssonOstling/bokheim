-- Cap the primary curated Economics & Business branch at two levels below its
-- root. Concepts shared outside the branch remain through their other parents.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

CREATE TEMP TABLE economics_tree(concept_id TEXT PRIMARY KEY,depth INTEGER NOT NULL) WITHOUT ROWID;
WITH RECURSIVE tree(concept_id,depth) AS (
 VALUES ('unified:103d333e-4f6d-5d1c-9c4b-950677c8d681',0)
 UNION ALL
 SELECT child.concept_id,tree.depth+1
 FROM tree
 JOIN concept_parent child ON child.parent_concept_id=tree.concept_id
)
INSERT INTO economics_tree
SELECT concept_id,min(depth) FROM tree GROUP BY concept_id;

CREATE TEMP TABLE economics_deep(concept_id TEXT PRIMARY KEY) WITHOUT ROWID;
INSERT INTO economics_deep
SELECT concept_id FROM economics_tree WHERE depth>=3;

CREATE TEMP TABLE economics_shared(concept_id TEXT PRIMARY KEY) WITHOUT ROWID;
WITH RECURSIVE preserved(concept_id) AS (
 SELECT deep.concept_id
 FROM economics_deep deep
 JOIN concept_parent edge ON edge.concept_id=deep.concept_id
 WHERE edge.parent_concept_id NOT IN (SELECT concept_id FROM economics_tree)
 UNION
 SELECT child.concept_id
 FROM preserved parent
 JOIN concept_parent child ON child.parent_concept_id=parent.concept_id
 WHERE child.concept_id IN (SELECT concept_id FROM economics_deep)
)
INSERT INTO economics_shared SELECT concept_id FROM preserved;

CREATE TEMP TABLE economics_remove(concept_id TEXT PRIMARY KEY) WITHOUT ROWID;
INSERT INTO economics_remove
SELECT concept_id FROM economics_deep
WHERE concept_id NOT IN (SELECT concept_id FROM economics_shared);

DELETE FROM concept_parent
WHERE parent_concept_id IN (
 SELECT concept_id FROM economics_tree WHERE depth=2
)
AND concept_id IN (SELECT concept_id FROM economics_deep);

DELETE FROM concept_parent
WHERE concept_id IN (SELECT concept_id FROM economics_remove)
   OR parent_concept_id IN (SELECT concept_id FROM economics_remove);
DELETE FROM concept WHERE concept_id IN (SELECT concept_id FROM economics_remove);

DROP TABLE economics_remove;
DROP TABLE economics_shared;
DROP TABLE economics_deep;
DROP TABLE economics_tree;
COMMIT;

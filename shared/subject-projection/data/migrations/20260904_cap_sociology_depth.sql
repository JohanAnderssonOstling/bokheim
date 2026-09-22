-- Cap the primary curated Sociology branch at two levels below Sociology.
-- Direct children may have children; those grandchildren are leaves. Concepts
-- shared outside Sociology remain available through their other parents.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

CREATE TEMP TABLE sociology_tree(concept_id TEXT PRIMARY KEY,depth INTEGER NOT NULL) WITHOUT ROWID;
WITH RECURSIVE tree(concept_id,depth) AS (
 VALUES ('unified:1dc4999e-4875-54c4-be21-e8b23a259be2',0)
 UNION ALL
 SELECT child.concept_id,tree.depth+1
 FROM tree
 JOIN concept_parent child ON child.parent_concept_id=tree.concept_id
)
INSERT INTO sociology_tree
SELECT concept_id,min(depth) FROM tree GROUP BY concept_id;

CREATE TEMP TABLE sociology_deep(concept_id TEXT PRIMARY KEY) WITHOUT ROWID;
INSERT INTO sociology_deep
SELECT concept_id FROM sociology_tree WHERE depth>=3;

CREATE TEMP TABLE sociology_shared(concept_id TEXT PRIMARY KEY) WITHOUT ROWID;
WITH RECURSIVE preserved(concept_id) AS (
 SELECT deep.concept_id
 FROM sociology_deep deep
 JOIN concept_parent edge ON edge.concept_id=deep.concept_id
 WHERE edge.parent_concept_id NOT IN (SELECT concept_id FROM sociology_tree)
 UNION
 SELECT child.concept_id
 FROM preserved parent
 JOIN concept_parent child ON child.parent_concept_id=parent.concept_id
 WHERE child.concept_id IN (SELECT concept_id FROM sociology_deep)
)
INSERT INTO sociology_shared SELECT concept_id FROM preserved;

CREATE TEMP TABLE sociology_remove(concept_id TEXT PRIMARY KEY) WITHOUT ROWID;
INSERT INTO sociology_remove
SELECT concept_id FROM sociology_deep
WHERE concept_id NOT IN (SELECT concept_id FROM sociology_shared);

-- Break every edge below the retained grandchildren.
DELETE FROM concept_parent
WHERE parent_concept_id IN (
 SELECT concept_id FROM sociology_tree WHERE depth=2
)
AND concept_id IN (SELECT concept_id FROM sociology_deep);

DELETE FROM concept_parent
WHERE concept_id IN (SELECT concept_id FROM sociology_remove)
   OR parent_concept_id IN (SELECT concept_id FROM sociology_remove);
DELETE FROM concept WHERE concept_id IN (SELECT concept_id FROM sociology_remove);

DROP TABLE sociology_remove;
DROP TABLE sociology_shared;
DROP TABLE sociology_deep;
DROP TABLE sociology_tree;
COMMIT;

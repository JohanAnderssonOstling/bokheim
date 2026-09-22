-- Cap the primary Social Sciences Anthropology branch at two levels below its
-- root. Preserve concepts independently reachable outside the branch.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

CREATE TEMP TABLE anthropology_tree(concept_id TEXT PRIMARY KEY,depth INTEGER NOT NULL) WITHOUT ROWID;
WITH RECURSIVE tree(concept_id,depth) AS (
 VALUES ('unified:382fd3a0-46d4-5262-9f64-e3d975fbad31',0)
 UNION ALL
 SELECT child.concept_id,tree.depth+1
 FROM tree JOIN concept_parent child ON child.parent_concept_id=tree.concept_id
)
INSERT INTO anthropology_tree SELECT concept_id,min(depth) FROM tree GROUP BY concept_id;

DELETE FROM concept_parent
WHERE rowid IN (
 SELECT edge.rowid FROM concept_parent edge
 JOIN anthropology_tree parent ON parent.concept_id=edge.parent_concept_id
 JOIN anthropology_tree child ON child.concept_id=edge.concept_id
 WHERE parent.depth+1>child.depth
);

CREATE TEMP TABLE anthropology_deep(concept_id TEXT PRIMARY KEY) WITHOUT ROWID;
WITH RECURSIVE tree(concept_id,depth) AS (
 VALUES ('unified:382fd3a0-46d4-5262-9f64-e3d975fbad31',0)
 UNION ALL
 SELECT child.concept_id,tree.depth+1
 FROM tree JOIN concept_parent child ON child.parent_concept_id=tree.concept_id
)
INSERT INTO anthropology_deep SELECT concept_id FROM tree WHERE depth>=3;

CREATE TEMP TABLE anthropology_shared(concept_id TEXT PRIMARY KEY) WITHOUT ROWID;
WITH RECURSIVE preserved(concept_id) AS (
 SELECT deep.concept_id FROM anthropology_deep deep
 JOIN concept_parent edge ON edge.concept_id=deep.concept_id
 WHERE edge.parent_concept_id NOT IN (SELECT concept_id FROM anthropology_tree)
 UNION
 SELECT child.concept_id FROM preserved parent
 JOIN concept_parent child ON child.parent_concept_id=parent.concept_id
 WHERE child.concept_id IN (SELECT concept_id FROM anthropology_deep)
)
INSERT INTO anthropology_shared SELECT concept_id FROM preserved;

CREATE TEMP TABLE anthropology_remove(concept_id TEXT PRIMARY KEY) WITHOUT ROWID;
INSERT INTO anthropology_remove SELECT concept_id FROM anthropology_deep
WHERE concept_id NOT IN (SELECT concept_id FROM anthropology_shared);

DELETE FROM concept_parent
WHERE parent_concept_id IN (
 SELECT concept_id FROM anthropology_tree WHERE depth=2
);
DELETE FROM concept_parent
WHERE concept_id IN (SELECT concept_id FROM anthropology_remove)
   OR parent_concept_id IN (SELECT concept_id FROM anthropology_remove);
DELETE FROM concept WHERE concept_id IN (SELECT concept_id FROM anthropology_remove);

DROP TABLE anthropology_remove;
DROP TABLE anthropology_shared;
DROP TABLE anthropology_deep;
DROP TABLE anthropology_tree;
COMMIT;

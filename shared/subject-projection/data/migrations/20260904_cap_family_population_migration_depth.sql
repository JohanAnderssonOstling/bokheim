-- Cap Family, Population & Migration at two levels below its curated root.
-- Preserve concepts independently reachable outside the branch.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

CREATE TEMP TABLE family_tree(concept_id TEXT PRIMARY KEY,depth INTEGER NOT NULL) WITHOUT ROWID;
WITH RECURSIVE tree(concept_id,depth) AS (
 VALUES ('unified:fe4a0f70-fd4b-581a-86b7-a07e3f99febc',0)
 UNION ALL
 SELECT child.concept_id,tree.depth+1
 FROM tree JOIN concept_parent child ON child.parent_concept_id=tree.concept_id
)
INSERT INTO family_tree SELECT concept_id,min(depth) FROM tree GROUP BY concept_id;

DELETE FROM concept_parent
WHERE rowid IN (
 SELECT edge.rowid FROM concept_parent edge
 JOIN family_tree parent ON parent.concept_id=edge.parent_concept_id
 JOIN family_tree child ON child.concept_id=edge.concept_id
 WHERE parent.depth+1>child.depth
);

CREATE TEMP TABLE family_deep(concept_id TEXT PRIMARY KEY) WITHOUT ROWID;
WITH RECURSIVE tree(concept_id,depth) AS (
 VALUES ('unified:fe4a0f70-fd4b-581a-86b7-a07e3f99febc',0)
 UNION ALL
 SELECT child.concept_id,tree.depth+1
 FROM tree JOIN concept_parent child ON child.parent_concept_id=tree.concept_id
)
INSERT INTO family_deep SELECT concept_id FROM tree WHERE depth>=3;

CREATE TEMP TABLE family_shared(concept_id TEXT PRIMARY KEY) WITHOUT ROWID;
WITH RECURSIVE preserved(concept_id) AS (
 SELECT deep.concept_id FROM family_deep deep
 JOIN concept_parent edge ON edge.concept_id=deep.concept_id
 WHERE edge.parent_concept_id NOT IN (SELECT concept_id FROM family_tree)
 UNION
 SELECT child.concept_id FROM preserved parent
 JOIN concept_parent child ON child.parent_concept_id=parent.concept_id
 WHERE child.concept_id IN (SELECT concept_id FROM family_deep)
)
INSERT INTO family_shared SELECT concept_id FROM preserved;

CREATE TEMP TABLE family_remove(concept_id TEXT PRIMARY KEY) WITHOUT ROWID;
INSERT INTO family_remove SELECT concept_id FROM family_deep
WHERE concept_id NOT IN (SELECT concept_id FROM family_shared);

DELETE FROM concept_parent
WHERE parent_concept_id IN (
 SELECT concept_id FROM family_tree WHERE depth=2
);
DELETE FROM concept_parent
WHERE concept_id IN (SELECT concept_id FROM family_remove)
   OR parent_concept_id IN (SELECT concept_id FROM family_remove);
DELETE FROM concept WHERE concept_id IN (SELECT concept_id FROM family_remove);

DROP TABLE family_remove;
DROP TABLE family_shared;
DROP TABLE family_deep;
DROP TABLE family_tree;
COMMIT;

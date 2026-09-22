-- Cap primary Politics & Government at two levels and remove redundant deeper
-- placements in both Politics and Economics that bypass the depth cap.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

CREATE TEMP TABLE politics_tree(concept_id TEXT PRIMARY KEY,depth INTEGER NOT NULL) WITHOUT ROWID;
WITH RECURSIVE tree(concept_id,depth) AS (
 VALUES ('unified:fa671db5-b0ef-5492-9c50-638c60fa5148',0)
 UNION ALL
 SELECT child.concept_id,tree.depth+1
 FROM tree JOIN concept_parent child ON child.parent_concept_id=tree.concept_id
)
INSERT INTO politics_tree SELECT concept_id,min(depth) FROM tree GROUP BY concept_id;

-- When the same concept has both shallow and deep placements, retain only the
-- shallow route. Otherwise its children would reappear below the depth cap.
DELETE FROM concept_parent
WHERE rowid IN (
 SELECT edge.rowid
 FROM concept_parent edge
 JOIN politics_tree parent ON parent.concept_id=edge.parent_concept_id
 JOIN politics_tree child ON child.concept_id=edge.concept_id
 WHERE parent.depth+1>child.depth
);

CREATE TEMP TABLE politics_deep(concept_id TEXT PRIMARY KEY) WITHOUT ROWID;
WITH RECURSIVE tree(concept_id,depth) AS (
 VALUES ('unified:fa671db5-b0ef-5492-9c50-638c60fa5148',0)
 UNION ALL
 SELECT child.concept_id,tree.depth+1
 FROM tree JOIN concept_parent child ON child.parent_concept_id=tree.concept_id
)
INSERT INTO politics_deep SELECT concept_id FROM tree WHERE depth>=3;

CREATE TEMP TABLE politics_shared(concept_id TEXT PRIMARY KEY) WITHOUT ROWID;
WITH RECURSIVE preserved(concept_id) AS (
 SELECT deep.concept_id FROM politics_deep deep
 JOIN concept_parent edge ON edge.concept_id=deep.concept_id
 WHERE edge.parent_concept_id NOT IN (SELECT concept_id FROM politics_tree)
 UNION
 SELECT child.concept_id FROM preserved parent
 JOIN concept_parent child ON child.parent_concept_id=parent.concept_id
 WHERE child.concept_id IN (SELECT concept_id FROM politics_deep)
)
INSERT INTO politics_shared SELECT concept_id FROM preserved;

CREATE TEMP TABLE politics_remove(concept_id TEXT PRIMARY KEY) WITHOUT ROWID;
INSERT INTO politics_remove SELECT concept_id FROM politics_deep
WHERE concept_id NOT IN (SELECT concept_id FROM politics_shared);

DELETE FROM concept_parent
WHERE parent_concept_id IN (
 SELECT concept_id FROM politics_tree WHERE depth=2
);
DELETE FROM concept_parent
WHERE concept_id IN (SELECT concept_id FROM politics_remove)
   OR parent_concept_id IN (SELECT concept_id FROM politics_remove);
DELETE FROM concept WHERE concept_id IN (SELECT concept_id FROM politics_remove);

-- Remove Economics placements that are deeper duplicates of a shallower route.
CREATE TEMP TABLE economics_depth(concept_id TEXT PRIMARY KEY,depth INTEGER NOT NULL) WITHOUT ROWID;
WITH RECURSIVE tree(concept_id,depth) AS (
 VALUES ('unified:103d333e-4f6d-5d1c-9c4b-950677c8d681',0)
 UNION ALL
 SELECT child.concept_id,tree.depth+1
 FROM tree JOIN concept_parent child ON child.parent_concept_id=tree.concept_id
)
INSERT INTO economics_depth SELECT concept_id,min(depth) FROM tree GROUP BY concept_id;
DELETE FROM concept_parent
WHERE rowid IN (
 SELECT edge.rowid FROM concept_parent edge
 JOIN economics_depth parent ON parent.concept_id=edge.parent_concept_id
 JOIN economics_depth child ON child.concept_id=edge.concept_id
 WHERE parent.depth+1>child.depth
);

DROP TABLE economics_depth;
DROP TABLE politics_remove;
DROP TABLE politics_shared;
DROP TABLE politics_deep;
DROP TABLE politics_tree;
COMMIT;

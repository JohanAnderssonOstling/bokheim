-- Flatten every By period wrapper in the curated History taxonomy. Periods
-- become direct children of the wrapper's nearest non-wrapper parent. Full
-- source structure remains in the master taxonomy.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

CREATE TEMP TABLE history_period_wrapper(concept_id TEXT PRIMARY KEY) WITHOUT ROWID;
WITH RECURSIVE history(concept_id) AS (
 VALUES ('bokheim:subject:history')
 UNION
 SELECT child.concept_id
 FROM concept_parent child
 JOIN history parent ON parent.concept_id=child.parent_concept_id
)
INSERT INTO history_period_wrapper
SELECT item.concept_id
FROM history item
JOIN concept c ON c.concept_id=item.concept_id
WHERE lower(c.preferred_label)='by period';

-- Follow wrapper-only chains so nested wrappers do not strand their children.
CREATE TEMP TABLE flattened_period_edge(
 concept_id TEXT NOT NULL,
 parent_concept_id TEXT NOT NULL,
 display_order INTEGER NOT NULL,
 PRIMARY KEY(concept_id,parent_concept_id)
) WITHOUT ROWID;
WITH RECURSIVE route(wrapper_id,parent_id,child_id,display_order) AS (
 SELECT wrapper.concept_id,parent.parent_concept_id,child.concept_id,child.display_order
 FROM history_period_wrapper wrapper
 JOIN concept_parent parent ON parent.concept_id=wrapper.concept_id
 JOIN concept_parent child ON child.parent_concept_id=wrapper.concept_id
 WHERE parent.parent_concept_id NOT IN (SELECT concept_id FROM history_period_wrapper)
 UNION ALL
 SELECT route.wrapper_id,route.parent_id,child.concept_id,child.display_order
 FROM route
 JOIN concept_parent child ON child.parent_concept_id=route.child_id
 WHERE route.child_id IN (SELECT concept_id FROM history_period_wrapper)
)
INSERT OR IGNORE INTO flattened_period_edge
SELECT child_id,parent_id,display_order
FROM route
WHERE child_id NOT IN (SELECT concept_id FROM history_period_wrapper)
  AND child_id<>parent_id;

CREATE TEMP TABLE new_period_edge AS
SELECT edge.concept_id,edge.parent_concept_id,edge.display_order,
       row_number() OVER (PARTITION BY edge.concept_id ORDER BY edge.parent_concept_id)-1 AS ordinal_offset
FROM flattened_period_edge edge
WHERE NOT EXISTS (
 SELECT 1 FROM concept_parent existing
 WHERE existing.concept_id=edge.concept_id
   AND existing.parent_concept_id=edge.parent_concept_id
);

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal,display_order)
SELECT edge.concept_id,edge.parent_concept_id,
       COALESCE((SELECT max(existing.ordinal)+1 FROM concept_parent existing
                 WHERE existing.concept_id=edge.concept_id),0)+edge.ordinal_offset,
       edge.display_order
FROM new_period_edge edge;

DELETE FROM concept_parent
WHERE concept_id IN (SELECT concept_id FROM history_period_wrapper)
   OR parent_concept_id IN (SELECT concept_id FROM history_period_wrapper);
DELETE FROM concept
WHERE concept_id IN (SELECT concept_id FROM history_period_wrapper);

DROP TABLE new_period_edge;
DROP TABLE flattened_period_edge;
DROP TABLE history_period_wrapper;
COMMIT;

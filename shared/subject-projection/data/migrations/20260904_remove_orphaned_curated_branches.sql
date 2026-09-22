-- Remove branches accidentally promoted to roots when their curated parent was
-- pruned. A valid curated root must also be a root in the master taxonomy.
-- Concepts shared with a valid branch survive because reachability is computed
-- across the complete remaining curated graph.

PRAGMA foreign_keys = ON;
ATTACH DATABASE 'master-taxonomy-v2.sqlite3' AS master;
BEGIN IMMEDIATE;

CREATE TEMP TABLE valid_root(concept_id TEXT PRIMARY KEY) WITHOUT ROWID;
INSERT INTO valid_root
SELECT concept.concept_id
FROM concept
WHERE NOT EXISTS (
        SELECT 1
        FROM concept_parent
        WHERE concept_parent.concept_id = concept.concept_id
    )
  AND NOT EXISTS (
        SELECT 1
        FROM master.concept_parent
        WHERE master.concept_parent.concept_id = concept.concept_id
    );

CREATE TEMP TABLE reachable(concept_id TEXT PRIMARY KEY) WITHOUT ROWID;
WITH RECURSIVE retained(concept_id) AS (
    SELECT concept_id FROM valid_root
    UNION
    SELECT edge.concept_id
    FROM retained
    JOIN concept_parent AS edge
      ON edge.parent_concept_id = retained.concept_id
)
INSERT INTO reachable SELECT concept_id FROM retained;

CREATE TEMP TABLE orphaned(concept_id TEXT PRIMARY KEY) WITHOUT ROWID;
INSERT INTO orphaned
SELECT concept_id
FROM concept
WHERE concept_id NOT IN (SELECT concept_id FROM reachable);

DELETE FROM concept_parent
WHERE concept_id IN (SELECT concept_id FROM orphaned)
   OR parent_concept_id IN (SELECT concept_id FROM orphaned);

DELETE FROM concept
WHERE concept_id IN (SELECT concept_id FROM orphaned);

DROP TABLE orphaned;
DROP TABLE reachable;
DROP TABLE valid_root;
COMMIT;
DETACH DATABASE master;

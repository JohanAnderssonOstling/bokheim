-- Andorra is the useful terminal browse subject; remove its deeper catalog
-- subdivisions from the curated taxonomy.

PRAGMA foreign_keys = ON;
ATTACH DATABASE 'master-taxonomy-v2.sqlite3' AS master;
BEGIN IMMEDIATE;

DELETE FROM concept_parent
WHERE parent_concept_id = 'unified:da3b8227-9e5e-5919-a851-cfa6c4922cdb';

CREATE TEMP TABLE valid_root(concept_id TEXT PRIMARY KEY) WITHOUT ROWID;
INSERT INTO valid_root
SELECT concept.concept_id
FROM concept
WHERE NOT EXISTS (
        SELECT 1 FROM concept_parent
        WHERE concept_parent.concept_id = concept.concept_id
    )
  AND NOT EXISTS (
        SELECT 1 FROM master.concept_parent
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

DELETE FROM concept_parent
WHERE concept_id NOT IN (SELECT concept_id FROM reachable)
   OR parent_concept_id NOT IN (SELECT concept_id FROM reachable);
DELETE FROM concept
WHERE concept_id NOT IN (SELECT concept_id FROM reachable);

DROP TABLE reachable;
DROP TABLE valid_root;
COMMIT;
DETACH DATABASE master;

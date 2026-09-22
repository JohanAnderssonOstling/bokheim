-- Replace the one-language Hellenic wrapper with Greek itself and remove the
-- historical-stage and linguistic-aspect detail below Greek.

PRAGMA foreign_keys = ON;
ATTACH DATABASE 'master-taxonomy-v2.sqlite3' AS master;
BEGIN IMMEDIATE;

UPDATE concept_parent
SET parent_concept_id = 'unified:a5bf4300-c76d-4c09-86aa-2eeee54db800'
WHERE concept_id = 'unified:1df321b8-5817-5494-9c08-a496ad538e39'
  AND parent_concept_id = 'unified:123d9ba8-f034-5738-8bda-0f2273b03712';

INSERT INTO source_selector(concept_id, system_id, selector, ordinal)
VALUES ('unified:1df321b8-5817-5494-9c08-a496ad538e39', 'ddc', '480', 0);

DELETE FROM concept_parent
WHERE parent_concept_id = 'unified:1df321b8-5817-5494-9c08-a496ad538e39';
DELETE FROM concept_parent
WHERE concept_id = 'unified:123d9ba8-f034-5738-8bda-0f2273b03712';
DELETE FROM concept
WHERE concept_id = 'unified:123d9ba8-f034-5738-8bda-0f2273b03712';

-- Delete the disconnected Greek detail while retaining any concept that is
-- still reachable through another curated branch.
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

-- Keep the direct curated categories for the Franco-Prussian War but make each
-- one a leaf. Finer military and political detail remains in the master taxonomy.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

CREATE TEMP TABLE franco_prussian_detail(concept_id TEXT PRIMARY KEY) WITHOUT ROWID;
WITH RECURSIVE detail(concept_id, depth) AS (
    SELECT child.concept_id, 1
    FROM concept_parent child
    WHERE child.parent_concept_id='bokheim:lcc-detail:2ed7ae64-1c47-5f80-b0b3-3d944240f22c'
    UNION
    SELECT child.concept_id, parent.depth + 1
    FROM detail parent
    JOIN concept_parent child ON child.parent_concept_id=parent.concept_id
)
INSERT INTO franco_prussian_detail
SELECT concept_id FROM detail WHERE depth > 1;

-- Preserve nodes shared with taxonomy branches outside the war hierarchy.
CREATE TEMP TABLE franco_prussian_shared(concept_id TEXT PRIMARY KEY) WITHOUT ROWID;
WITH RECURSIVE shared(concept_id) AS (
    SELECT detail.concept_id
    FROM franco_prussian_detail detail
    JOIN concept_parent edge ON edge.concept_id=detail.concept_id
    WHERE edge.parent_concept_id NOT IN (SELECT concept_id FROM franco_prussian_detail)
      AND edge.parent_concept_id NOT IN (
          SELECT concept_id FROM concept_parent
          WHERE parent_concept_id='bokheim:lcc-detail:2ed7ae64-1c47-5f80-b0b3-3d944240f22c'
      )
    UNION
    SELECT child.concept_id
    FROM shared parent
    JOIN concept_parent child ON child.parent_concept_id=parent.concept_id
    WHERE child.concept_id IN (SELECT concept_id FROM franco_prussian_detail)
)
INSERT INTO franco_prussian_shared SELECT concept_id FROM shared;

CREATE TEMP TABLE franco_prussian_remove(concept_id TEXT PRIMARY KEY) WITHOUT ROWID;
INSERT INTO franco_prussian_remove
SELECT concept_id FROM franco_prussian_detail
WHERE concept_id NOT IN (SELECT concept_id FROM franco_prussian_shared);

DELETE FROM concept_parent
WHERE concept_id IN (SELECT concept_id FROM franco_prussian_remove)
   OR parent_concept_id IN (SELECT concept_id FROM franco_prussian_remove);
DELETE FROM concept WHERE concept_id IN (SELECT concept_id FROM franco_prussian_remove);

DROP TABLE franco_prussian_remove;
DROP TABLE franco_prussian_shared;
DROP TABLE franco_prussian_detail;
COMMIT;

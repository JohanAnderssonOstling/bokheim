-- Make every direct entry under France > By period a curated leaf. The full
-- chronology and topical breakdown remain in the master taxonomy.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

CREATE TEMP TABLE france_period_detail(concept_id TEXT PRIMARY KEY) WITHOUT ROWID;
WITH RECURSIVE detail(concept_id,depth) AS (
    SELECT child.concept_id,1
    FROM concept_parent child
    WHERE child.parent_concept_id='bokheim:france-history:by-period'
    UNION
    SELECT child.concept_id,parent.depth+1
    FROM detail parent
    JOIN concept_parent child ON child.parent_concept_id=parent.concept_id
)
INSERT INTO france_period_detail
SELECT concept_id FROM detail WHERE depth>1;

-- Preserve a descendant if another curated branch also owns it, together with
-- any of its descendants inside the period detail set.
CREATE TEMP TABLE france_period_shared(concept_id TEXT PRIMARY KEY) WITHOUT ROWID;
WITH RECURSIVE shared(concept_id) AS (
    SELECT detail.concept_id
    FROM france_period_detail detail
    JOIN concept_parent edge ON edge.concept_id=detail.concept_id
    WHERE edge.parent_concept_id NOT IN (SELECT concept_id FROM france_period_detail)
      AND edge.parent_concept_id NOT IN (
          SELECT concept_id FROM concept_parent
          WHERE parent_concept_id='bokheim:france-history:by-period'
      )
    UNION
    SELECT child.concept_id
    FROM shared parent
    JOIN concept_parent child ON child.parent_concept_id=parent.concept_id
    WHERE child.concept_id IN (SELECT concept_id FROM france_period_detail)
)
INSERT INTO france_period_shared SELECT concept_id FROM shared;

CREATE TEMP TABLE france_period_remove(concept_id TEXT PRIMARY KEY) WITHOUT ROWID;
INSERT INTO france_period_remove
SELECT concept_id FROM france_period_detail
WHERE concept_id NOT IN (SELECT concept_id FROM france_period_shared);

DELETE FROM concept_parent
WHERE concept_id IN (SELECT concept_id FROM france_period_remove)
   OR parent_concept_id IN (SELECT concept_id FROM france_period_remove);
DELETE FROM concept WHERE concept_id IN (SELECT concept_id FROM france_period_remove);

DROP TABLE france_period_remove;
DROP TABLE france_period_shared;
DROP TABLE france_period_detail;
COMMIT;

-- Repair first-child links skipped by ordinal conflicts.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

WITH moves(child_id,group_id) AS (VALUES
 (4153,54908),(4154,54908),(4155,54908),
 (4190,54909),(4191,54909),(4245,54909),(12702,54909),(12703,54909)
)
INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT child_id,group_id,
       COALESCE((SELECT MAX(ordinal) FROM concept_parent x WHERE x.parent_concept_id=group_id),-1)+1
FROM moves;

COMMIT;

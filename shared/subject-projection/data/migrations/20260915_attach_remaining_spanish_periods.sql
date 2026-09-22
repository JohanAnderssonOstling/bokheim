-- Attach the remaining regional Spanish period entries to By Period groups.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

WITH moves(child_id,group_id) AS (VALUES
 (11951,54899),(11956,54900),(11965,54902),(11978,54905)
)
INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT child_id,group_id,
       COALESCE((SELECT MAX(ordinal) FROM concept_parent x WHERE x.parent_concept_id=group_id),-1)+1
FROM moves;

COMMIT;

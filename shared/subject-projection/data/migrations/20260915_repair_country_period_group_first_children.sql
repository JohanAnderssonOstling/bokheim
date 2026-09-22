-- Restore first period children skipped by the initial grouping migration.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

WITH moves(child_id,group_id) AS (VALUES
 (12718,54875),(11914,54876),(3870,54877),(3900,54878),(4162,54879),
 (4169,54880),(4185,54881),(4195,54882),(4199,54883),(12707,54884),
 (4217,54885),(4221,54886),(4227,54887),(12347,54888),(12536,54889),
 (12636,54890),(4390,54891),(4395,54892),(4474,54893),
 (11926,54895),(11930,54896),(11934,54897),(11938,54898),(52010,54907)
)
INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT child_id,group_id,
       COALESCE((SELECT MAX(ordinal) FROM concept_parent x WHERE x.parent_concept_id=group_id),-1)+1
FROM moves;

COMMIT;

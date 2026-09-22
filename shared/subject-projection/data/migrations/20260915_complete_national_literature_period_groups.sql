-- Complete By Period coverage for the remaining national literature branches.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

INSERT OR IGNORE INTO concept(concept_id,preferred_label) VALUES
 (54908,'By Period'),(54909,'By Period');

INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT 54908,4152,COALESCE((SELECT MAX(ordinal) FROM concept_parent WHERE parent_concept_id=4152),-1)+1;
INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT 54909,4244,COALESCE((SELECT MAX(ordinal) FROM concept_parent WHERE parent_concept_id=4244),-1)+1;

WITH moves(child_id,group_id) AS (VALUES
 (4153,54908),(4154,54908),(4155,54908),
 (4190,54909),(4191,54909),(4245,54909),(12702,54909),(12703,54909),
 (12535,54889),(12491,54885)
)
INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT child_id,group_id,
       COALESCE((SELECT MAX(ordinal) FROM concept_parent x WHERE x.parent_concept_id=group_id),-1)+1
FROM moves;

DELETE FROM concept_parent
WHERE (parent_concept_id=4152 AND concept_id IN (4153,4154,4155))
   OR (parent_concept_id=4244 AND concept_id IN (4190,4191,4245,12702,12703))
   OR (parent_concept_id=4268 AND concept_id=12535)
   OR (parent_concept_id=4216 AND concept_id=12491);

COMMIT;

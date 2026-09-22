-- Repair national literature period entries missed by the period grouping.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

WITH moves(child_id,group_id) AS (VALUES
 (11950,54899),(11955,54900),(11960,54901),(11964,54902),
 (11969,54903),(11973,54904),(11977,54905),(11982,54906)
)
INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT child_id,group_id,COALESCE((SELECT MAX(ordinal) FROM concept_parent x WHERE x.parent_concept_id=group_id),-1)+1
FROM moves;

DELETE FROM concept_parent
WHERE (concept_id=11950 AND parent_concept_id=11949)
   OR (concept_id=11955 AND parent_concept_id=11954)
   OR (concept_id=11960 AND parent_concept_id=11959)
   OR (concept_id=11964 AND parent_concept_id=11963)
   OR (concept_id=11969 AND parent_concept_id=11968)
   OR (concept_id=11973 AND parent_concept_id=11972)
   OR (concept_id=11977 AND parent_concept_id=11976)
   OR (concept_id=11982 AND parent_concept_id=11981);

INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT 4231,5590,COALESCE((SELECT MAX(ordinal) FROM concept_parent WHERE parent_concept_id=5590),-1)+1;

COMMIT;

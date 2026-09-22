-- A code must resolve to its most specific curated subject. If a direct child
-- owns the identical exact LCC selector, remove the duplicate from its parent.
PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;

DELETE FROM source_selector AS parent_selector
WHERE parent_selector.system_id='lcc'
  AND instr(parent_selector.selector,'..')=0
  AND EXISTS (
    SELECT 1
    FROM concept_parent cp
    JOIN source_selector child_selector
      ON child_selector.concept_id=cp.concept_id
     AND child_selector.system_id=parent_selector.system_id
     AND child_selector.selector=parent_selector.selector
    WHERE cp.parent_concept_id=parent_selector.concept_id
  );

COMMIT;

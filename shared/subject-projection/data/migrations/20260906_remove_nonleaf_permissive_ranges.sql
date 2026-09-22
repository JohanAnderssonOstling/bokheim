-- Non-leaf concepts may own exact selectors, but permissive LCC ranges belong
-- only on leaves. Descendant selectors remain unchanged and provide the
-- specific routing; uncovered schedule gaps are deliberately not inferred.
PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

DELETE FROM source_selector AS selector
WHERE selector.system_id = 'lcc'
  AND EXISTS (
    SELECT 1
    FROM concept_parent AS child
    WHERE child.parent_concept_id = selector.concept_id
  )
  AND selector.selector LIKE '%..%'
  AND substr(selector.selector, 1, instr(selector.selector, '..') - 1)
      <> substr(
        substr(selector.selector, instr(selector.selector, '..') + 2),
        1,
        CASE
          WHEN instr(substr(selector.selector, instr(selector.selector, '..') + 2), '@') > 0
          THEN instr(substr(selector.selector, instr(selector.selector, '..') + 2), '@') - 1
          ELSE length(substr(selector.selector, instr(selector.selector, '..') + 2))
        END
      );

COMMIT;

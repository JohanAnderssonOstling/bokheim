-- Internal concepts may own exact selectors and therefore have direct hits.
-- Only terminal concepts may own permissive selectors: wildcards or genuine
-- ranges spanning more than one classification code. Zero-width ranges are
-- exact selectors and are normalized by the following migration.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

DELETE FROM source_selector AS selector
WHERE EXISTS (
 SELECT 1
 FROM concept_parent AS child
 WHERE child.parent_concept_id=selector.concept_id
)
AND (
 selector.selector LIKE '%*%'
 OR (
  selector.selector LIKE '%..%'
  AND substr(selector.selector, 1, instr(selector.selector, '..') - 1)
      <> substr(
       substr(selector.selector, instr(selector.selector, '..') + 2),
       1,
       CASE
        WHEN instr(substr(selector.selector, instr(selector.selector, '..') + 2), '@') > 0
        THEN instr(substr(selector.selector, instr(selector.selector, '..') + 2), '@') - 1
        ELSE length(substr(selector.selector, instr(selector.selector, '..') + 2))
       END
      )
 )
);

COMMIT;

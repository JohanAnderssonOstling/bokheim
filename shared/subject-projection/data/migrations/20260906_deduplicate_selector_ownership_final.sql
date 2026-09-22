PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;

-- A selector key has exactly one owner. Prefer a leaf, then the deepest placement,
-- then the stable concept id as a deterministic tie-breaker.
WITH RECURSIVE
 roots(concept_id,depth) AS (
   SELECT c.concept_id,0 FROM concept c
   WHERE NOT EXISTS (SELECT 1 FROM concept_parent p WHERE p.concept_id=c.concept_id)
   UNION ALL
   SELECT p.concept_id,roots.depth+1 FROM roots JOIN concept_parent p ON p.parent_concept_id=roots.concept_id
 ),
 depth AS (SELECT concept_id,max(depth) depth FROM roots GROUP BY concept_id),
 ranked AS (
   SELECT s.rowid rid,
          row_number() OVER (
            PARTITION BY s.system_id,s.selector
            ORDER BY
              CASE WHEN EXISTS (SELECT 1 FROM concept_parent p WHERE p.parent_concept_id=s.concept_id) THEN 1 ELSE 0 END,
              coalesce(d.depth,0) DESC,
              s.concept_id
          ) keep_rank
   FROM source_selector s LEFT JOIN depth d USING(concept_id)
 )
DELETE FROM source_selector WHERE rowid IN (SELECT rid FROM ranked WHERE keep_rank>1);

COMMIT;

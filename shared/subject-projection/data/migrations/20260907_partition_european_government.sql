-- European Government is navigational.  Its JN schedule is partitioned by
-- the existing country/region leaves, so the parent must not retain an
-- overlapping permissive range.

INSERT INTO lcc_range(start_code,end_code,concept_id)
VALUES
  ('JN101','JN1371',11598),
  ('JN3201','JN4980',11604);

DELETE FROM lcc_selector
WHERE (concept_id,selector) IN (
  (11598,'JN101-1371'),
  (11604,'JN3201-4980')
);

-- Every exact selector formerly attached to the European umbrella falls in
-- exactly one existing child range after adding Great Britain and Germany.
UPDATE lcc_selector AS selector
SET concept_id = (
  SELECT ranges.concept_id
  FROM lcc_range AS ranges
  JOIN concept_parent AS edge USING(concept_id)
  WHERE edge.parent_concept_id=5368
    AND CAST(substr(selector.selector,3) AS REAL)
        BETWEEN CAST(substr(ranges.start_code,3) AS REAL)
            AND CAST(substr(ranges.end_code,3) AS REAL)
)
WHERE selector.concept_id=5368
  AND selector.selector GLOB 'JN[0-9]*';

DELETE FROM lcc_range WHERE concept_id=5368;

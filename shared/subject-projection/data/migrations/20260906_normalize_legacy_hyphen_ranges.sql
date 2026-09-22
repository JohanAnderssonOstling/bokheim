-- Canonicalize legacy LCC start-end selectors to the taxonomy's start..end
-- notation. Abbreviated numeric endpoints inherit the 1-3 letter class from
-- the start (QA1-939 -> QA1..QA939). Selectors already containing ".." are
-- excluded because a trailing A-Z there describes a cutter suffix.
PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;

CREATE TEMP TABLE _legacy_lcc_range(
  concept_id INTEGER NOT NULL,
  old_selector TEXT NOT NULL,
  new_selector TEXT NOT NULL,
  PRIMARY KEY(concept_id,old_selector)
) WITHOUT ROWID;

INSERT INTO _legacy_lcc_range
WITH parts AS (
  SELECT concept_id,
         selector,
         substr(selector,1,instr(selector,'-')-1) AS range_start,
         substr(selector,instr(selector,'-')+1) AS range_end
  FROM source_selector
  WHERE system_id='lcc'
    AND selector LIKE '%-%'
    AND selector NOT LIKE '%..%'
), normalized AS (
  SELECT concept_id,selector,range_start,range_end,
         CASE
           WHEN substr(range_start,3,1) GLOB '[A-Z]' THEN substr(range_start,1,3)
           WHEN substr(range_start,2,1) GLOB '[A-Z]' THEN substr(range_start,1,2)
           ELSE substr(range_start,1,1)
         END AS class_prefix
  FROM parts
)
SELECT concept_id,selector,
       range_start || '..' ||
       CASE WHEN substr(range_end,1,1) GLOB '[A-Z]'
            THEN range_end ELSE class_prefix || range_end END
FROM normalized;

INSERT OR IGNORE INTO source_selector(concept_id,system_id,selector)
SELECT concept_id,'lcc',new_selector FROM _legacy_lcc_range;

DELETE FROM source_selector
WHERE system_id='lcc' AND EXISTS (
  SELECT 1 FROM _legacy_lcc_range r
  WHERE r.concept_id=source_selector.concept_id
    AND r.old_selector=source_selector.selector
);

DROP TABLE _legacy_lcc_range;
COMMIT;

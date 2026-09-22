-- Remove publication and study-form filing headings while retaining their
-- classification coverage on the immediate meaningful subject.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

CREATE TEMP TABLE form_heading(
    concept_id TEXT PRIMARY KEY,
    parent_id TEXT NOT NULL
) WITHOUT ROWID;

INSERT INTO form_heading
SELECT heading.concept_id, edge.parent_concept_id
FROM concept heading
JOIN concept_parent edge ON edge.concept_id = heading.concept_id
WHERE lower(heading.preferred_label) LIKE 'handbooks%tables%etc.%'
   OR lower(heading.preferred_label) LIKE 'pocketbooks%tables%etc.%'
   OR lower(heading.preferred_label) LIKE 'examinations, questions%etc.%'
   OR lower(heading.preferred_label) = 'pamphlets, etc.'
   OR lower(heading.preferred_label) LIKE 'problems, exercises%etc.%'
   OR lower(heading.preferred_label) LIKE 'tables, calculations%etc.%'
   OR lower(heading.preferred_label) LIKE 'dealers'' catalogs%etc.%'
   OR lower(heading.preferred_label) LIKE 'lists and catalogs of supplies%etc.%'
   OR lower(heading.preferred_label) LIKE 'collections of monographs, essays%etc.%'
   OR lower(heading.preferred_label) LIKE 'addresses, essays%etc.%'
   OR lower(heading.preferred_label) LIKE 'addresses, essays, lectures%';

-- If a meaningful child is already attached to the destination, discard the
-- redundant edge before promoting the remaining children.
DELETE FROM concept_parent AS child_edge
WHERE child_edge.parent_concept_id IN (SELECT concept_id FROM form_heading)
  AND EXISTS (
      SELECT 1
      FROM form_heading heading
      JOIN concept_parent existing
        ON existing.concept_id = child_edge.concept_id
       AND existing.parent_concept_id = heading.parent_id
      WHERE heading.concept_id = child_edge.parent_concept_id
  );

UPDATE concept_parent AS child_edge
SET parent_concept_id = (
    SELECT heading.parent_id
    FROM form_heading heading
    WHERE heading.concept_id = child_edge.parent_concept_id
)
WHERE child_edge.parent_concept_id IN (SELECT concept_id FROM form_heading);

WITH moved AS (
    SELECT heading.parent_id, selector.system_id, selector.selector,
           row_number() OVER (
               PARTITION BY heading.parent_id, selector.system_id
               ORDER BY heading.concept_id, selector.ordinal
           ) AS offset
    FROM form_heading heading
    JOIN source_selector selector ON selector.concept_id = heading.concept_id
), base AS (
    SELECT moved.parent_id, moved.system_id,
           COALESCE(max(existing.ordinal) + 1, 0) AS first_ordinal
    FROM moved
    LEFT JOIN source_selector existing
      ON existing.concept_id = moved.parent_id
     AND existing.system_id = moved.system_id
    GROUP BY moved.parent_id, moved.system_id
)
INSERT OR IGNORE INTO source_selector(concept_id, system_id, selector, ordinal)
SELECT moved.parent_id, moved.system_id, moved.selector,
       base.first_ordinal + moved.offset - 1
FROM moved
JOIN base
  ON base.parent_id = moved.parent_id
 AND base.system_id = moved.system_id;

DELETE FROM concept
WHERE concept_id IN (SELECT concept_id FROM form_heading);

DROP TABLE form_heading;
COMMIT;

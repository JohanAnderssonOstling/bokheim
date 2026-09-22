-- Nomenclature/terminology/notation/abbreviation headings are generic forms,
-- not useful standalone browse subjects. Preserve their ranges on the
-- corresponding scientific parent.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

CREATE TEMP TABLE nomenclature_leaf(
    concept_id TEXT PRIMARY KEY,
    parent_id TEXT NOT NULL
) WITHOUT ROWID;

INSERT INTO nomenclature_leaf
SELECT child.concept_id,edge.parent_concept_id
FROM concept child
JOIN concept_parent edge ON edge.concept_id=child.concept_id
WHERE lower(child.preferred_label)='nomenclature, terminology, notation, abbreviations';

WITH moved AS (
    SELECT leaf.parent_id,s.system_id,s.selector,
           row_number() OVER (
               PARTITION BY leaf.parent_id,s.system_id
               ORDER BY leaf.concept_id,s.ordinal
           ) AS offset
    FROM nomenclature_leaf leaf
    JOIN source_selector s ON s.concept_id=leaf.concept_id
)
INSERT OR IGNORE INTO source_selector(concept_id,system_id,selector,ordinal)
SELECT moved.parent_id,moved.system_id,moved.selector,
       COALESCE((SELECT max(existing.ordinal)+1
                 FROM source_selector existing
                 WHERE existing.concept_id=moved.parent_id
                   AND existing.system_id=moved.system_id),0)
       + moved.offset - 1
FROM moved;

DELETE FROM concept_parent
WHERE concept_id IN (SELECT concept_id FROM nomenclature_leaf);

DELETE FROM concept
WHERE concept_id IN (SELECT concept_id FROM nomenclature_leaf);

DROP TABLE nomenclature_leaf;
COMMIT;

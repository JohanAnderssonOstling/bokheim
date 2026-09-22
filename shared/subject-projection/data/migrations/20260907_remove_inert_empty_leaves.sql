-- Remove leaf concepts that have no route from any supported source system.
-- This deliberately retains selector-free internal/navigation concepts.
BEGIN;

-- Also repairs a partially applied run made by a connection without
-- PRAGMA foreign_keys enabled.
DELETE FROM concept_parent
WHERE NOT EXISTS (SELECT 1 FROM concept WHERE concept_id = concept_parent.concept_id)
   OR NOT EXISTS (SELECT 1 FROM concept WHERE concept_id = concept_parent.parent_concept_id);

CREATE TEMP TABLE _inert_leaf(concept_id INTEGER PRIMARY KEY);
INSERT INTO _inert_leaf
SELECT concept.concept_id
FROM concept
WHERE NOT EXISTS (
        SELECT 1 FROM concept_parent
        WHERE parent_concept_id = concept.concept_id
    )
  AND NOT EXISTS (
        SELECT 1 FROM lcc_selector
        WHERE concept_id = concept.concept_id
    )
  AND NOT EXISTS (
        SELECT 1 FROM lcc_range
        WHERE concept_id = concept.concept_id
    )
  AND NOT EXISTS (
        SELECT 1 FROM bisac_selector
        WHERE concept_id = concept.concept_id
    )
  AND NOT EXISTS (
        SELECT 1 FROM source_label
        WHERE concept_id = concept.concept_id
    );

DELETE FROM concept_parent
WHERE concept_id IN (SELECT concept_id FROM _inert_leaf);

DELETE FROM concept
WHERE concept_id IN (SELECT concept_id FROM _inert_leaf);

DROP TABLE _inert_leaf;

COMMIT;

-- The six top-level Biography & Autobiography BISAC groupings below are
-- presentation-only wrappers. Promote their children to the coded root and
-- retain Forms, General & Reference because it has BISAC BIO012000.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

INSERT OR IGNORE INTO concept_parent (concept_id, parent_concept_id)
SELECT child.concept_id, 3212
FROM concept_parent child
WHERE child.parent_concept_id IN (5765, 5766, 5767, 5768, 5769, 5770);

DELETE FROM concept_parent
WHERE concept_id IN (5765, 5766, 5767, 5768, 5769, 5770)
   OR parent_concept_id IN (5765, 5766, 5767, 5768, 5769, 5770);

DELETE FROM lcc_selector WHERE concept_id IN (5765, 5766, 5767, 5768, 5769, 5770);
DELETE FROM bisac_selector WHERE concept_id IN (5765, 5766, 5767, 5768, 5769, 5770);
DELETE FROM ddc_selector WHERE concept_id IN (5765, 5766, 5767, 5768, 5769, 5770);
DELETE FROM source_label WHERE concept_id IN (5765, 5766, 5767, 5768, 5769, 5770);
DELETE FROM concept WHERE concept_id IN (5765, 5766, 5767, 5768, 5769, 5770);

COMMIT;

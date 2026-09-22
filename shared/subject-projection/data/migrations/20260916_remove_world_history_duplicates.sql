-- Remove duplicate World links and merge World Ancient/Military into the
-- existing thematic concepts.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

-- World Military is the duplicate of the History-root Military concept.
UPDATE concept_parent SET parent_concept_id = 3640
WHERE parent_concept_id = 10300;
UPDATE lcc_range SET concept_id = 3640 WHERE concept_id = 10300;
INSERT OR IGNORE INTO lcc_selector(concept_id, selector)
SELECT 3640, selector FROM lcc_selector WHERE concept_id = 10300;
DELETE FROM lcc_selector WHERE concept_id = 10300;
DELETE FROM bisac_selector WHERE concept_id = 10300;
DELETE FROM source_label WHERE concept_id = 10300;
DELETE FROM concept_parent WHERE concept_id = 10300;
DELETE FROM concept WHERE concept_id = 10300;

-- World Ancient is the duplicate of Eras Ancient.
UPDATE concept_parent SET parent_concept_id = 3480
WHERE parent_concept_id = 10720;
UPDATE lcc_range SET concept_id = 3480 WHERE concept_id = 10720;
INSERT OR IGNORE INTO lcc_selector(concept_id, selector)
SELECT 3480, selector FROM lcc_selector WHERE concept_id = 10720;
DELETE FROM lcc_selector WHERE concept_id = 10720;
DELETE FROM bisac_selector WHERE concept_id = 10720;
DELETE FROM source_label WHERE concept_id = 10720;
DELETE FROM concept_parent WHERE concept_id = 10720;
DELETE FROM concept WHERE concept_id = 10720;

-- These concepts already have their thematic parent under History/Eras.
DELETE FROM concept_parent
WHERE (concept_id, parent_concept_id) IN ((3620,3701),(3608,3701),(3666,3701));

-- Eastern Hemisphere is a geographic grouping, so place it alongside the
-- other regional groupings rather than under the World LCC container.
UPDATE concept_parent
SET parent_concept_id = 5540
WHERE concept_id = 13691 AND parent_concept_id = 3701;

COMMIT;

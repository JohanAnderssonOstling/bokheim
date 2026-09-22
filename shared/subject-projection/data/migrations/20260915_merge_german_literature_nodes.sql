-- Merge the BISAC German subject into the existing LCC German & Related
-- Literatures hierarchy. Keep the LCC concept and use the short name German.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

UPDATE concept SET preferred_label='German' WHERE concept_id=4244;

INSERT OR IGNORE INTO bisac_selector (concept_id,selector)
SELECT 4244,selector FROM bisac_selector WHERE concept_id=4189;

CREATE TEMP TABLE german_bisac_children (concept_id INTEGER PRIMARY KEY);
INSERT INTO german_bisac_children (concept_id)
SELECT concept_id FROM concept_parent WHERE parent_concept_id=4189;

DELETE FROM concept_parent
WHERE concept_id IN (SELECT concept_id FROM german_bisac_children)
   OR parent_concept_id=4189;

INSERT INTO concept_parent (concept_id,parent_concept_id,ordinal)
SELECT concept_id,4244,0 FROM german_bisac_children;

DELETE FROM bisac_selector WHERE concept_id=4189;
DELETE FROM lcc_selector WHERE concept_id=4189;
DELETE FROM ddc_selector WHERE concept_id=4189;
DELETE FROM source_label WHERE concept_id=4189;
DELETE FROM concept_parent WHERE concept_id=4189 OR parent_concept_id=4189;
DELETE FROM concept WHERE concept_id=4189;

DROP TABLE german_bisac_children;
COMMIT;

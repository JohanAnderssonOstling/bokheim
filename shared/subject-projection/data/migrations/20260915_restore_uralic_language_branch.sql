-- Restore Uralic & Basque children after moving the group out of Slavic &
-- Eurasian, and merge the duplicate Finnish and Hungarian country nodes.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

UPDATE concept SET preferred_label='Uralic & Basque' WHERE concept_id=4479;

INSERT OR IGNORE INTO concept_parent (concept_id,parent_concept_id,ordinal) VALUES
  (4480,4479,0),(4481,4479,0),(4482,4479,0),(4483,4479,0),
  (4484,4479,0),(4485,4479,0),(4486,4479,0);

INSERT OR IGNORE INTO bisac_selector (concept_id,selector)
SELECT 4482,selector FROM bisac_selector WHERE concept_id=4183;
INSERT OR IGNORE INTO bisac_selector (concept_id,selector)
SELECT 4483,selector FROM bisac_selector WHERE concept_id=4193;

DELETE FROM concept_parent WHERE concept_id IN (4183,4193);
DELETE FROM lcc_selector WHERE concept_id IN (4183,4193);
DELETE FROM bisac_selector WHERE concept_id IN (4183,4193);
DELETE FROM ddc_selector WHERE concept_id IN (4183,4193);
DELETE FROM source_label WHERE concept_id IN (4183,4193);
DELETE FROM concept WHERE concept_id IN (4183,4193);

COMMIT;

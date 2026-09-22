-- Merge country biography nodes that already existed under European Biographies.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

INSERT OR IGNORE INTO lcc_selector(concept_id,selector)
SELECT 13435,selector FROM lcc_selector WHERE concept_id=51894;
INSERT OR IGNORE INTO bisac_selector(concept_id,selector)
SELECT 13435,selector FROM bisac_selector WHERE concept_id=51894;
DELETE FROM concept_parent WHERE concept_id=51894 OR parent_concept_id=51894;
DELETE FROM concept WHERE concept_id=51894;

INSERT OR IGNORE INTO lcc_selector(concept_id,selector)
SELECT 13412,selector FROM lcc_selector WHERE concept_id=51903;
INSERT OR IGNORE INTO bisac_selector(concept_id,selector)
SELECT 13412,selector FROM bisac_selector WHERE concept_id=51903;
DELETE FROM concept_parent WHERE concept_id=51903 OR parent_concept_id=51903;
DELETE FROM concept WHERE concept_id=51903;

INSERT OR IGNORE INTO lcc_selector(concept_id,selector)
SELECT 13436,selector FROM lcc_selector WHERE concept_id=51914;
INSERT OR IGNORE INTO bisac_selector(concept_id,selector)
SELECT 13436,selector FROM bisac_selector WHERE concept_id=51914;
DELETE FROM concept_parent WHERE concept_id=51914 OR parent_concept_id=51914;
DELETE FROM concept WHERE concept_id=51914;

COMMIT;

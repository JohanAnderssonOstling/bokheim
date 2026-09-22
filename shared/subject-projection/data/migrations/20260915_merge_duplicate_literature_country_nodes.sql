-- Merge duplicate country nodes created by the LCC and BISAC literature
-- branches. Keep the node with the existing child hierarchy and transfer the
-- BISAC selector from its duplicate.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

INSERT OR IGNORE INTO bisac_selector (concept_id,selector) VALUES
  (3869,'FIC097000'),
  (3899,'FIC112000'),
  (4394,'FIC114000'),
  (4473,'FIC125000');

DELETE FROM bisac_selector WHERE concept_id IN (4174,4209,4211,4230);
DELETE FROM lcc_selector WHERE concept_id IN (4174,4209,4211,4230);
DELETE FROM ddc_selector WHERE concept_id IN (4174,4209,4211,4230);
DELETE FROM source_label WHERE concept_id IN (4174,4209,4211,4230);
DELETE FROM concept_parent WHERE concept_id IN (4174,4209,4211,4230)
   OR parent_concept_id IN (4174,4209,4211,4230);
DELETE FROM concept WHERE concept_id IN (4174,4209,4211,4230);

COMMIT;

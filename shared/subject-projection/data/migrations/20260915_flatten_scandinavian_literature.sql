-- Scandinavian Literature is an uncoded wrapper under Germanic & Nordic.
-- Promote its country and tradition children directly to that LCC-aligned
-- group.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

CREATE TEMP TABLE scandinavian_children (concept_id INTEGER PRIMARY KEY);
INSERT INTO scandinavian_children (concept_id)
SELECT concept_id FROM concept_parent WHERE parent_concept_id=4459;

DELETE FROM concept_parent
WHERE concept_id IN (SELECT concept_id FROM scandinavian_children)
   OR parent_concept_id=4459;

INSERT INTO concept_parent (concept_id,parent_concept_id,ordinal)
SELECT concept_id,5587,0 FROM scandinavian_children;

DELETE FROM lcc_selector WHERE concept_id=4459;
DELETE FROM bisac_selector WHERE concept_id=4459;
DELETE FROM ddc_selector WHERE concept_id=4459;
DELETE FROM source_label WHERE concept_id=4459;
DELETE FROM concept_parent WHERE concept_id=4459 OR parent_concept_id=4459;
DELETE FROM concept WHERE concept_id=4459;

DROP TABLE scandinavian_children;
COMMIT;

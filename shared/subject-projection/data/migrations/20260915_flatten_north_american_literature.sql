-- North American Literature is an uncoded wrapper. Its country subjects are
-- specific BISAC leaves and belong directly under American & Caribbean.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

CREATE TEMP TABLE north_american_children (concept_id INTEGER PRIMARY KEY);
INSERT INTO north_american_children (concept_id)
SELECT concept_id FROM concept_parent WHERE parent_concept_id=5728;

DELETE FROM concept_parent
WHERE concept_id IN (SELECT concept_id FROM north_american_children)
   OR parent_concept_id=5728;

INSERT INTO concept_parent (concept_id,parent_concept_id,ordinal)
SELECT concept_id,5727,0 FROM north_american_children;

DELETE FROM lcc_selector WHERE concept_id=5728;
DELETE FROM bisac_selector WHERE concept_id=5728;
DELETE FROM ddc_selector WHERE concept_id=5728;
DELETE FROM source_label WHERE concept_id=5728;
DELETE FROM concept_parent WHERE concept_id=5728 OR parent_concept_id=5728;
DELETE FROM concept WHERE concept_id=5728;

DROP TABLE north_american_children;
COMMIT;

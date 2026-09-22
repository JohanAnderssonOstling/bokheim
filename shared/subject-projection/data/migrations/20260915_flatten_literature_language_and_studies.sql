-- Literature is the root for its language/tradition and study subjects.
-- Remove the two presentation-only wrappers while retaining their children.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

CREATE TEMP TABLE literature_promoted_children (concept_id INTEGER PRIMARY KEY);
INSERT INTO literature_promoted_children (concept_id)
SELECT concept_id FROM concept_parent WHERE parent_concept_id IN (11224,5584);

DELETE FROM concept_parent
WHERE concept_id IN (SELECT concept_id FROM literature_promoted_children)
   OR parent_concept_id IN (11224,5584);

INSERT INTO concept_parent (concept_id,parent_concept_id,ordinal)
SELECT concept_id,3846,0 FROM literature_promoted_children;

DELETE FROM lcc_selector WHERE concept_id IN (11224,5584);
DELETE FROM bisac_selector WHERE concept_id IN (11224,5584);
DELETE FROM ddc_selector WHERE concept_id IN (11224,5584);
DELETE FROM source_label WHERE concept_id IN (11224,5584);
DELETE FROM concept_parent WHERE concept_id IN (11224,5584) OR parent_concept_id IN (11224,5584);
DELETE FROM concept WHERE concept_id IN (11224,5584);

DROP TABLE literature_promoted_children;
COMMIT;

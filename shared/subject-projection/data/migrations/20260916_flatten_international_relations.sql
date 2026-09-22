-- Flatten the redundant International Relations Foundations wrapper.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

INSERT OR IGNORE INTO lcc_selector(concept_id,selector)
SELECT 5693,selector FROM lcc_selector WHERE concept_id=5352;
INSERT OR IGNORE INTO bisac_selector(concept_id,selector)
SELECT 5693,selector FROM bisac_selector WHERE concept_id=5352;

UPDATE concept
SET preferred_label=CASE concept_id
  WHEN 12889 THEN 'Foundations'
  WHEN 12890 THEN 'Theory'
  WHEN 12891 THEN 'By Period'
  WHEN 12893 THEN 'By Region'
  ELSE preferred_label END
WHERE concept_id IN (12889,12890,12891,12893);

DELETE FROM concept_parent WHERE concept_id IN (12889,12890,12891,12892,12893);
INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT concept_id,5693,ROW_NUMBER() OVER (ORDER BY concept_id)-1
FROM concept
WHERE concept_id IN (12889,12890,12891,12892,12893);

DELETE FROM concept_parent WHERE concept_id=5352 OR parent_concept_id=5352;
DELETE FROM lcc_selector WHERE concept_id=5352;
DELETE FROM bisac_selector WHERE concept_id=5352;
DELETE FROM source_label WHERE concept_id=5352;
DELETE FROM concept WHERE concept_id=5352;

COMMIT;

PRAGMA foreign_keys=ON;
BEGIN;

-- DDC 794 and GV1312-GV1469 describe the same board/strategy-game area.
-- Keep the LCC-backed concept and move the richer curated subtree onto it.
INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT 53242,parent_concept_id,ordinal
FROM concept_parent
WHERE concept_id=5902;

INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT concept_id,53242,ordinal
FROM concept_parent
WHERE parent_concept_id=5902;

UPDATE ddc_selector SET concept_id=53242 WHERE selector='794';
DELETE FROM concept_parent WHERE parent_concept_id=5902;
DELETE FROM concept WHERE concept_id=5902;

-- The prayer schedule is a narrower part of the existing Worship range.
INSERT OR REPLACE INTO lcc_range(start_code,end_code,concept_id)
VALUES('BV205','BV287',11470);

-- DDC 131 is occult mental training, not the history of psychology methods.
UPDATE ddc_selector SET concept_id=5423 WHERE selector='131';

COMMIT;

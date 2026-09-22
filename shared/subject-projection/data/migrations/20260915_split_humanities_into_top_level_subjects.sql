-- Humanities is only a wrapper in the curated taxonomy. Its P–PZ range is
-- specifically the Language and Literature LCC span, so keep that coverage
-- on Language while promoting the existing subject branches to root level.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

UPDATE lcc_range
SET concept_id=3702
WHERE concept_id=3211
  AND start_code='P'
  AND end_code='PZ';

DELETE FROM concept_parent WHERE parent_concept_id=3211;
DELETE FROM source_label WHERE concept_id=3211;
DELETE FROM concept WHERE concept_id=3211;

COMMIT;

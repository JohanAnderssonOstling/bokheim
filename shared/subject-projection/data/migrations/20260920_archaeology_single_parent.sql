-- Keep general Archaeology directly under Social Sciences only.
-- Requested removal of its secondary Anthropology placement.
PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;
DELETE FROM concept_parent WHERE concept_id=5492 AND parent_concept_id=5056;
COMMIT;

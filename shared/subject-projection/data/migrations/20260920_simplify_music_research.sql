-- Combine Musical research with its broader music studies/reference parent.
-- Preserve the surviving ID and all selectors and children.
PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;
UPDATE concept SET preferred_label='Music Research & Reference' WHERE concept_id=782;
UPDATE lcc_range SET concept_id=782 WHERE concept_id=796;
UPDATE concept_parent SET parent_concept_id=782 WHERE parent_concept_id=796;
DELETE FROM concept WHERE concept_id=796;
COMMIT;

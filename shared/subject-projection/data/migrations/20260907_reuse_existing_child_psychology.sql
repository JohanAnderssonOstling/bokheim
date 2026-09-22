-- Reuse existing Child psychology subject; a concurrent extraction duplicated it.
BEGIN IMMEDIATE;
UPDATE lcc_range SET concept_id=10989 WHERE concept_id=52698;
DELETE FROM concept_parent WHERE concept_id=52698;
DELETE FROM concept WHERE concept_id=52698;
COMMIT;

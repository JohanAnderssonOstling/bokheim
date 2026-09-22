-- Preserve the primary children’s Games & Activities placement and the general Card Games route.
BEGIN;
DELETE FROM concept_parent WHERE concept_id=1506 AND parent_concept_id=4665 AND ordinal=1;
COMMIT;

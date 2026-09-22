BEGIN;

DELETE FROM lcc_range WHERE concept_id=13273;
DELETE FROM lcc_selector WHERE concept_id=13273;
DELETE FROM bisac_selector WHERE concept_id=13273;
DELETE FROM source_label WHERE concept_id=13273;
DELETE FROM concept_parent WHERE concept_id=13273 OR parent_concept_id=13273;
DELETE FROM concept WHERE concept_id=13273;

COMMIT;

BEGIN IMMEDIATE;

-- Leave room for the promoted children ahead of the remaining theology groups.
UPDATE concept_parent
SET display_order = display_order + 100
WHERE parent_concept_id = 'unified:a75623fc-9deb-5c8b-afa9-da87a8734211'
  AND concept_id <> 'bokheim:christian-theology:god-creation';

-- Promote the complete child set while preserving its existing order.
UPDATE concept_parent
SET parent_concept_id = 'unified:a75623fc-9deb-5c8b-afa9-da87a8734211'
WHERE parent_concept_id = 'bokheim:christian-theology:god-creation';

DELETE FROM concept_parent
WHERE concept_id = 'bokheim:christian-theology:god-creation';

DELETE FROM source_label
WHERE concept_id = 'bokheim:christian-theology:god-creation';

DELETE FROM source_selector
WHERE concept_id = 'bokheim:christian-theology:god-creation';

DELETE FROM concept
WHERE concept_id = 'bokheim:christian-theology:god-creation';

COMMIT;

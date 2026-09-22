-- Language and Literature are represented as separate curated roots. The
-- combined LCC wrapper is empty and redundant because Language retains the
-- broad P coverage while Literature carries its detailed literature classes.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

DELETE FROM lcc_selector WHERE concept_id=53069;
DELETE FROM bisac_selector WHERE concept_id=53069;
DELETE FROM ddc_selector WHERE concept_id=53069;
DELETE FROM source_label WHERE concept_id=53069;
DELETE FROM concept_parent
WHERE concept_id=53069 OR parent_concept_id=53069;
DELETE FROM concept WHERE concept_id=53069;

COMMIT;

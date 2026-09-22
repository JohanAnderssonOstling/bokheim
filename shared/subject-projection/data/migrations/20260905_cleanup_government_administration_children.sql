PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;

-- Public Affairs & Administration is a redundant one-selector leaf. Its broad
-- selector belongs directly to the Government & Administration parent.
UPDATE source_selector
SET concept_id='bokheim:politics:government-administration', ordinal=(
 SELECT COALESCE(max(s.ordinal),-1)+1
 FROM source_selector s
 WHERE s.concept_id='bokheim:politics:government-administration'
   AND s.system_id=source_selector.system_id
)
WHERE concept_id='unified:92d435f9-29c2-5118-9a40-b65c6e315543';
DELETE FROM concept WHERE concept_id='unified:92d435f9-29c2-5118-9a40-b65c6e315543';

-- This child has neither selectors nor descendants.
DELETE FROM concept
WHERE concept_id='unified:bc0e260e-f208-55ac-8f74-ffbc18d2c84c'
  AND NOT EXISTS (SELECT 1 FROM source_selector s WHERE s.concept_id=concept.concept_id)
  AND NOT EXISTS (SELECT 1 FROM concept_parent cp WHERE cp.parent_concept_id=concept.concept_id);

COMMIT;

BEGIN;

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT 52693,52692,COALESCE(MAX(ordinal),-1)+1 FROM concept_parent
WHERE concept_id=52693 AND NOT EXISTS(
 SELECT 1 FROM concept_parent WHERE concept_id=52693 AND parent_concept_id=52692);
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT 52471,52470,COALESCE(MAX(ordinal),-1)+1 FROM concept_parent
WHERE concept_id=52471 AND NOT EXISTS(
 SELECT 1 FROM concept_parent WHERE concept_id=52471 AND parent_concept_id=52470);
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT 52472,52470,COALESCE(MAX(ordinal),-1)+1 FROM concept_parent
WHERE concept_id=52472 AND NOT EXISTS(
 SELECT 1 FROM concept_parent WHERE concept_id=52472 AND parent_concept_id=52470);

-- Remove the selectorless duplicate created before the existing Monetary
-- Unions concept was detected as the range owner.
DELETE FROM concept_parent WHERE concept_id=52795 OR parent_concept_id=52795;
DELETE FROM source_label WHERE concept_id=52795;
DELETE FROM lcc_selector WHERE concept_id=52795;
DELETE FROM bisac_selector WHERE concept_id=52795;
DELETE FROM lcc_range WHERE concept_id=52795;
DELETE FROM concept WHERE concept_id=52795;

COMMIT;

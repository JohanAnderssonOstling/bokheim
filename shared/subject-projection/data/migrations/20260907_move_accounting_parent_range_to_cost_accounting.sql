BEGIN;

INSERT INTO concept(preferred_label)
SELECT 'Cost & Industrial Accounting'
WHERE NOT EXISTS (
    SELECT 1 FROM concept WHERE preferred_label='Cost & Industrial Accounting'
);

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT c.concept_id,5105,
       COALESCE((SELECT max(ordinal)+1 FROM concept_parent WHERE parent_concept_id=5105),0)
FROM concept c
WHERE c.preferred_label='Cost & Industrial Accounting'
  AND NOT EXISTS (
      SELECT 1 FROM concept_parent cp
      WHERE cp.concept_id=c.concept_id AND cp.parent_concept_id=5105
  );

UPDATE lcc_range
SET concept_id=(SELECT concept_id FROM concept WHERE preferred_label='Cost & Industrial Accounting')
WHERE concept_id=5105 AND start_code='HD47' AND end_code='HD47.4';

COMMIT;

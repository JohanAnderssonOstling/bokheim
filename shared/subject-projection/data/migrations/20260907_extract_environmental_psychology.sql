BEGIN;

INSERT INTO concept(preferred_label) VALUES ('Environmental Psychology');

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT concept_id,12875,0 FROM concept
WHERE preferred_label='Environmental Psychology';

UPDATE lcc_selector
SET concept_id=(SELECT concept_id FROM concept WHERE preferred_label='Environmental Psychology')
WHERE concept_id=12875 AND selector='BF353';

COMMIT;

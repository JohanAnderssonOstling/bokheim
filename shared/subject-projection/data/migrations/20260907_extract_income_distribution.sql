BEGIN;

INSERT INTO concept(preferred_label)
VALUES ('Income Distribution & Distributive Justice');

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT concept_id,13128,0
FROM concept WHERE preferred_label='Income Distribution & Distributive Justice';

UPDATE lcc_selector
SET concept_id=(SELECT concept_id FROM concept WHERE preferred_label='Income Distribution & Distributive Justice')
WHERE concept_id=13128 AND selector IN ('HB523','HC79.I5');

COMMIT;

BEGIN;

INSERT INTO concept(preferred_label) VALUES ('Urban Ecology');

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT concept_id,12304,0 FROM concept WHERE preferred_label='Urban Ecology';

UPDATE lcc_selector
SET concept_id=(SELECT concept_id FROM concept WHERE preferred_label='Urban Ecology')
WHERE concept_id=12304 AND selector='HT241';

COMMIT;

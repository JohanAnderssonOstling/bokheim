BEGIN;

UPDATE concept SET preferred_label='Migration Studies'
WHERE concept_id=12441 AND preferred_label='Migration Policy & Studies';

INSERT INTO concept(preferred_label) VALUES ('Migration Policy');

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT concept_id,12441,0 FROM concept WHERE preferred_label='Migration Policy';

UPDATE lcc_selector
SET concept_id=(SELECT concept_id FROM concept WHERE preferred_label='Migration Policy')
WHERE concept_id=12441 AND selector='JV6038';

COMMIT;

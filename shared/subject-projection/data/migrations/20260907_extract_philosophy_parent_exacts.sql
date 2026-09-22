BEGIN;
WITH wanted(label) AS (VALUES ('Conscience'),('Otherness in Philosophy'))
INSERT INTO concept(preferred_label) SELECT label FROM wanted
WHERE NOT EXISTS (SELECT 1 FROM concept WHERE preferred_label=label);
INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT concept_id,7534,0 FROM concept WHERE preferred_label='Conscience';
INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT concept_id,4536,0 FROM concept WHERE preferred_label='Otherness in Philosophy';
UPDATE lcc_selector SET concept_id=(SELECT concept_id FROM concept WHERE preferred_label='Conscience')
WHERE concept_id=7534 AND selector='BJ1471';
UPDATE lcc_selector SET concept_id=(SELECT concept_id FROM concept WHERE preferred_label='Otherness in Philosophy')
WHERE concept_id=4536 AND selector='BD460.O74';
COMMIT;

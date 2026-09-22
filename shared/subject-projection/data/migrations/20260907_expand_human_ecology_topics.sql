BEGIN;

INSERT INTO concept(preferred_label) VALUES
 ('Human Influence on the Environment'),
 ('Urban Geography');

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT concept_id,13086,0 FROM concept
WHERE preferred_label='Human Influence on the Environment';

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT concept_id,13087,0 FROM concept
WHERE preferred_label='Urban Geography';

UPDATE lcc_selector
SET concept_id=(SELECT concept_id FROM concept WHERE preferred_label='Human Influence on the Environment')
WHERE concept_id=5066 AND selector='GF75';

UPDATE lcc_selector
SET concept_id=(SELECT concept_id FROM concept WHERE preferred_label='Urban Geography')
WHERE concept_id=5066 AND selector='GF125';

UPDATE lcc_selector SET concept_id=13086
WHERE concept_id=5066 AND selector='GF51';

UPDATE lcc_selector SET concept_id=13087
WHERE concept_id=5066 AND selector IN ('GF101','GF101-GF127','GF127');

INSERT OR IGNORE INTO lcc_selector(concept_id,selector)
VALUES (5066,'GF1-GF900');

DELETE FROM lcc_range
WHERE concept_id=5066 AND start_code='GF1' AND end_code='GF900';

COMMIT;

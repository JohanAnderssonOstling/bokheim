BEGIN;

CREATE TEMP TABLE _cognition_topic(label TEXT NOT NULL, selector TEXT NOT NULL);
INSERT INTO _cognition_topic VALUES
 ('Unconscious & Subconscious Mind','BF315'),
 ('Intuition','BF315.5');

INSERT INTO concept(preferred_label)
SELECT label FROM _cognition_topic;

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT c.concept_id,12873,0
FROM _cognition_topic t JOIN concept c ON c.preferred_label=t.label;

UPDATE lcc_selector
SET concept_id=(SELECT concept_id FROM concept WHERE preferred_label='Unconscious & Subconscious Mind')
WHERE concept_id=12873 AND selector='BF315';

UPDATE lcc_selector
SET concept_id=(SELECT concept_id FROM concept WHERE preferred_label='Intuition')
WHERE concept_id=12873 AND selector='BF315.5';

DROP TABLE _cognition_topic;
COMMIT;

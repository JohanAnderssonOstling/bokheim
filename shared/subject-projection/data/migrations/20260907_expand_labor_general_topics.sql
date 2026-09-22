BEGIN;

CREATE TEMP TABLE _labor_topic(label TEXT NOT NULL, selector TEXT NOT NULL);
INSERT INTO _labor_topic VALUES
 ('Work & Family','HD4904.25'),
 ('Human Capital','HD4904.7');

INSERT INTO concept(preferred_label)
SELECT label FROM _labor_topic;

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT c.concept_id,12354,0
FROM _labor_topic t JOIN concept c ON c.preferred_label=t.label;

UPDATE lcc_selector
SET concept_id=(SELECT concept_id FROM concept WHERE preferred_label='Work & Family')
WHERE concept_id=12354 AND selector='HD4904.25';

UPDATE lcc_selector
SET concept_id=(SELECT concept_id FROM concept WHERE preferred_label='Human Capital')
WHERE concept_id=12354 AND selector='HD4904.7';

DROP TABLE _labor_topic;
COMMIT;

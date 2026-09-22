BEGIN;

CREATE TEMP TABLE _organization_topic(label TEXT NOT NULL, selector TEXT NOT NULL);
INSERT INTO _organization_topic VALUES
 ('Conflict Management','HD42'),
 ('Workplace Negotiation & Persuasion','HD58.6');

INSERT INTO concept(preferred_label)
SELECT label FROM _organization_topic t
WHERE NOT EXISTS (SELECT 1 FROM concept c WHERE c.preferred_label=t.label);

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT c.concept_id,11593,0
FROM _organization_topic t JOIN concept c ON c.preferred_label=t.label
WHERE NOT EXISTS (
 SELECT 1 FROM concept_parent cp
 WHERE cp.concept_id=c.concept_id AND cp.parent_concept_id=11593
);

UPDATE lcc_selector
SET concept_id=(SELECT c.concept_id FROM concept c WHERE c.preferred_label='Conflict Management')
WHERE concept_id=11593 AND selector='HD42';

UPDATE lcc_selector
SET concept_id=(SELECT c.concept_id FROM concept c WHERE c.preferred_label='Workplace Negotiation & Persuasion')
WHERE concept_id=11593 AND selector='HD58.6';

UPDATE lcc_selector SET concept_id=52710
WHERE concept_id=11593 AND selector='HD50-50.5';

UPDATE lcc_selector SET concept_id=52711
WHERE concept_id=11593 AND selector='HD58.9';

UPDATE lcc_selector SET concept_id=52712
WHERE concept_id=11593 AND selector IN ('HD66-66.5','HD66.7');

DROP TABLE _organization_topic;
COMMIT;

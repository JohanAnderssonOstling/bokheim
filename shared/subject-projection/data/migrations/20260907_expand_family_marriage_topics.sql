BEGIN;

CREATE TEMP TABLE _family_topic(label TEXT NOT NULL, selector TEXT NOT NULL);
INSERT INTO _family_topic VALUES
 ('Adultery','HQ806'),
 ('Mixed & Interfaith Marriage','HQ1031');

INSERT INTO concept(preferred_label) SELECT label FROM _family_topic;

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT c.concept_id,5252,0
FROM _family_topic t JOIN concept c ON c.preferred_label=t.label;

UPDATE lcc_selector
SET concept_id=(SELECT concept_id FROM concept WHERE preferred_label='Adultery')
WHERE concept_id=5252 AND selector='HQ806';

UPDATE lcc_selector
SET concept_id=(SELECT concept_id FROM concept WHERE preferred_label='Mixed & Interfaith Marriage')
WHERE concept_id=5252 AND selector='HQ1031';

UPDATE lcc_selector SET concept_id=11801
WHERE concept_id=5252 AND selector IN ('HQ520','HQ744','HQ746');

UPDATE lcc_selector SET concept_id=12287
WHERE concept_id=5252 AND selector IN
 ('HQ802','HQ802.5','HQ803','HQ803.5','HQ805');

UPDATE lcc_selector SET concept_id=12290
WHERE concept_id=5252 AND selector='HQ997';

UPDATE lcc_selector SET concept_id=12292
WHERE concept_id=5252 AND selector IN
 ('HQ1017','HQ1026','HQ1028','HQ1032');

DROP TABLE _family_topic;
COMMIT;

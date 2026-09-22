BEGIN;

INSERT INTO concept(preferred_label) VALUES ('Women & Economics');

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT concept_id,12252,0 FROM concept WHERE preferred_label='Women & Economics';

UPDATE lcc_selector
SET concept_id=(SELECT concept_id FROM concept WHERE preferred_label='Women & Economics')
WHERE concept_id=5253 AND selector='HQ1381';

UPDATE lcc_selector SET concept_id=12247
WHERE concept_id=5253 AND selector IN ('HQ1106','HQ1115');

UPDATE lcc_selector SET concept_id=12248
WHERE concept_id=5253 AND selector='HQ1190-HQ1194';

UPDATE lcc_selector SET concept_id=12249
WHERE concept_id=5253 AND selector IN ('HQ1201-HQ1229','HQ1233');

UPDATE lcc_selector SET concept_id=5379
WHERE concept_id=5253 AND selector='HQ1236-HQ1236.5';

UPDATE lcc_selector SET concept_id=12250
WHERE concept_id=5253 AND selector='HQ1237-HQ1237.5';

UPDATE lcc_selector SET concept_id=12251
WHERE concept_id=5253 AND selector='HQ1240-HQ1240.5';

UPDATE lcc_selector SET concept_id=12252
WHERE concept_id=5253 AND selector IN
 ('HQ1389','HQ1390-HQ1391','HQ1393-HQ1395','HQ1397','HQ1399.5');

COMMIT;

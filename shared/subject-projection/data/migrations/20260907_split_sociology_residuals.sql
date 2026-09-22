BEGIN;

WITH wanted(label) AS (VALUES
 ('Culture & Social Progress'),
 ('Social Groups & Association'))
INSERT INTO concept(preferred_label)
SELECT label FROM wanted
WHERE NOT EXISTS (SELECT 1 FROM concept WHERE preferred_label=label);

-- Culture/progress spans both the culture and social-change browsing arms.
INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT concept_id,5662,0 FROM concept WHERE preferred_label='Culture & Social Progress';
INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT concept_id,5665,1 FROM concept WHERE preferred_label='Culture & Social Progress';
INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT concept_id,5661,0 FROM concept WHERE preferred_label='Social Groups & Association';

UPDATE lcc_selector SET concept_id=13838
WHERE concept_id=5488 AND selector IN
 ('HM1','HM1.A','HM1.Z','HM7','HM41','HM43');
UPDATE lcc_selector SET concept_id=12260
WHERE concept_id=5488 AND selector IN ('HM24','HM37','HM201');
UPDATE lcc_selector SET concept_id=12321
WHERE concept_id=5488 AND selector IN ('HM66','HM68');
UPDATE lcc_selector SET concept_id=5662
WHERE concept_id=5488 AND selector IN ('HM44','HM44.5');
UPDATE lcc_selector SET concept_id=(SELECT concept_id FROM concept WHERE preferred_label='Culture & Social Progress')
WHERE concept_id=5488 AND selector IN ('HM101','HM108','HM121');

UPDATE lcc_range SET concept_id=(SELECT concept_id FROM concept WHERE preferred_label='Social Groups & Association')
WHERE concept_id=5488 AND class_letters='HM' AND start_value='131' AND end_value='134';

COMMIT;

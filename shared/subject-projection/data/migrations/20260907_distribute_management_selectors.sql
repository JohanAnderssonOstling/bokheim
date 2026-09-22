BEGIN;

WITH wanted(label) AS (VALUES ('Operations Management'),('History of Management'))
INSERT INTO concept(preferred_label)
SELECT label FROM wanted
WHERE NOT EXISTS (SELECT 1 FROM concept WHERE preferred_label=label);
INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT concept_id,5236,0 FROM concept
WHERE preferred_label='Operations Management';
INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT concept_id,5236,0 FROM concept
WHERE preferred_label='History of Management';

-- Strategy and general management.
UPDATE lcc_selector SET concept_id=11589
WHERE concept_id=5236 AND selector IN ('HD31','HD38','HD41');
UPDATE lcc_range SET concept_id=11589
WHERE concept_id=5236 AND class_letters='HD' AND (
 (start_value='30.19' AND end_value='30.29') OR
 (start_value='31' AND end_value='37') OR
 (start_value='38' AND end_value='38.5'));

-- History of management.
UPDATE lcc_range
SET concept_id=(SELECT concept_id FROM concept WHERE preferred_label='History of Management')
WHERE concept_id=5236 AND class_letters='HD'
  AND start_value='30.5' AND end_value='30.65';

-- Capital, revenue, and industrial costs.
UPDATE lcc_selector SET concept_id=5142
WHERE concept_id=5236 AND selector IN ('HD39','HD60.7');
UPDATE lcc_range SET concept_id=5142
WHERE concept_id=5236 AND class_letters='HD'
  AND start_value='39' AND end_value='40.7';
UPDATE lcc_selector SET concept_id=5105
WHERE concept_id=5236 AND selector='HD47';
UPDATE lcc_range SET concept_id=5105
WHERE concept_id=5236 AND class_letters='HD'
  AND start_value='47' AND end_value='47.4';

-- Business intelligence and intellectual capital.
UPDATE lcc_selector SET concept_id=11591
WHERE concept_id=5236 AND selector IN ('HD38.7','HD53');

-- Organizational structure, conflict, delegation, and negotiation.
UPDATE lcc_selector SET concept_id=11593
WHERE concept_id=5236 AND selector IN
 ('HD42','HD50','HD51','HD57.8','HD58.4','HD58.5','HD58.6');
UPDATE lcc_range SET concept_id=11593
WHERE concept_id=5236 AND class_letters='HD'
  AND start_value='50' AND end_value='50.5';

-- Location of industry belongs with Industries, not general management.
UPDATE lcc_selector SET concept_id=5157
WHERE concept_id=5236 AND selector='HD58';

-- Standardization, support services, and workflow form a coherent operations
-- leaf and no longer need to remain as miscellaneous management selectors.
UPDATE lcc_selector
SET concept_id=(SELECT concept_id FROM concept WHERE preferred_label='Operations Management')
WHERE concept_id=5236 AND selector IN ('HD62','HD62.17');
UPDATE lcc_range
SET concept_id=(SELECT concept_id FROM concept WHERE preferred_label='Operations Management')
WHERE concept_id=5236 AND class_letters='HD' AND (
 (start_value='62.12' AND end_value='62.13') OR
 (start_value='62.2' AND end_value='62.8'));

COMMIT;

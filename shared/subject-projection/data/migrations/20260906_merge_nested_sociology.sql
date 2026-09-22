-- Merge nested Sociology into the main Sociology branch; remove the empty nested Social Sciences node.
PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;
UPDATE source_selector SET concept_id='unified:1dc4999e-4875-54c4-be21-e8b23a259be2',ordinal=1 WHERE concept_id='unified:38ac9c54-af17-54e5-aa17-0613f079c100' AND system_id='bisac' AND selector='SOC026000';
UPDATE source_selector SET concept_id='unified:1dc4999e-4875-54c4-be21-e8b23a259be2',ordinal=2 WHERE concept_id='unified:38ac9c54-af17-54e5-aa17-0613f079c100' AND system_id='bisac' AND selector='SOC026010';
UPDATE source_selector SET concept_id='unified:1dc4999e-4875-54c4-be21-e8b23a259be2',ordinal=3 WHERE concept_id='unified:38ac9c54-af17-54e5-aa17-0613f079c100' AND system_id='bisac' AND selector='SOC026020';
UPDATE source_selector SET concept_id='unified:1dc4999e-4875-54c4-be21-e8b23a259be2',ordinal=4 WHERE concept_id='unified:38ac9c54-af17-54e5-aa17-0613f079c100' AND system_id='bisac' AND selector='SOC026030';
UPDATE source_selector SET concept_id='unified:1dc4999e-4875-54c4-be21-e8b23a259be2',ordinal=5 WHERE concept_id='unified:38ac9c54-af17-54e5-aa17-0613f079c100' AND system_id='bisac' AND selector='SOC026040';
UPDATE source_selector SET concept_id='unified:1dc4999e-4875-54c4-be21-e8b23a259be2',ordinal=1 WHERE concept_id='unified:38ac9c54-af17-54e5-aa17-0613f079c100' AND system_id='lcc' AND selector='HM';
UPDATE source_selector SET concept_id='unified:1dc4999e-4875-54c4-be21-e8b23a259be2',ordinal=2 WHERE concept_id='unified:38ac9c54-af17-54e5-aa17-0613f079c100' AND system_id='lcc' AND selector='HM1..HM1281@1';
DELETE FROM concept WHERE concept_id IN ('unified:38ac9c54-af17-54e5-aa17-0613f079c100','unified:548ba4d8-5ee2-5f3e-85a2-7b5e84c68fb1');
COMMIT;

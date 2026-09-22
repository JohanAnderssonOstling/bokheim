-- Absorb three low-hit literature leaves into parents, replacing nine selectors with three ranges.
PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;
INSERT INTO source_selector(concept_id,system_id,selector) VALUES(3931,'lcc','PT7581..PT7592@0');
INSERT INTO source_selector(concept_id,system_id,selector) VALUES(4473,'lcc','PT9509..PT9542@0');
INSERT INTO source_selector(concept_id,system_id,selector) VALUES(3899,'lcc','PT5351..PT5395@0');
DELETE FROM source_selector WHERE concept_id=3932;
DELETE FROM source_label WHERE concept_id=3932;
DELETE FROM concept_parent WHERE concept_id=3932;
DELETE FROM concept WHERE concept_id=3932;
DELETE FROM source_selector WHERE concept_id=4475;
DELETE FROM source_label WHERE concept_id=4475;
DELETE FROM concept_parent WHERE concept_id=4475;
DELETE FROM concept WHERE concept_id=4475;
DELETE FROM source_selector WHERE concept_id=3902;
DELETE FROM source_label WHERE concept_id=3902;
DELETE FROM concept_parent WHERE concept_id=3902;
DELETE FROM concept WHERE concept_id=3902;
COMMIT;

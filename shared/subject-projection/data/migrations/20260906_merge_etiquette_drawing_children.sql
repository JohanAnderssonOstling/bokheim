-- Absorb three low-hit etiquette/drawing leaves into parents; replace the backwards BJ range with ordered travel-etiquette coverage.
PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;
INSERT INTO source_selector(concept_id,system_id,selector) VALUES(4539,'lcc','BJ2137..BJ2156@0');
INSERT INTO source_selector(concept_id,system_id,selector) VALUES(668,'lcc','NC70..NC75@0');
INSERT INTO source_selector(concept_id,system_id,selector) VALUES(666,'lcc','NC855..NC875@0');
DELETE FROM source_selector WHERE concept_id=4541;
DELETE FROM source_label WHERE concept_id=4541;
DELETE FROM concept_parent WHERE concept_id=4541;
DELETE FROM concept WHERE concept_id=4541;
DELETE FROM source_selector WHERE concept_id=6364;
DELETE FROM source_label WHERE concept_id=6364;
DELETE FROM concept_parent WHERE concept_id=6364;
DELETE FROM concept WHERE concept_id=6364;
DELETE FROM source_selector WHERE concept_id=8192;
DELETE FROM source_label WHERE concept_id=8192;
DELETE FROM concept_parent WHERE concept_id=8192;
DELETE FROM concept WHERE concept_id=8192;
COMMIT;

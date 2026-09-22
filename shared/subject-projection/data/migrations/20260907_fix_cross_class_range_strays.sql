BEGIN;

UPDATE lcc_range SET concept_id=5884
WHERE concept_id=13341 AND start_code IN ('S589.8','S589.85');

UPDATE lcc_range SET concept_id=7393
WHERE concept_id=13339 AND start_code IN ('S770','S775','S776','S785','S790');

INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal)
VALUES (7393,12,1);

UPDATE lcc_range SET concept_id=5793
WHERE concept_id=13348 AND start_code='SF111';

UPDATE lcc_range SET concept_id=5793
WHERE concept_id=13352 AND start_code IN ('SF114','SF117');

UPDATE lcc_range SET concept_id=595
WHERE concept_id=13541 AND start_code IN ('NA2101','NA2300');

UPDATE lcc_range SET concept_id=1065
WHERE concept_id=13559 AND start_code IN ('NC390','NC610');

UPDATE lcc_range SET concept_id=306
WHERE concept_id=13495 AND start_code='TD940';

COMMIT;

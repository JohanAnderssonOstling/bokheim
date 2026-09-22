BEGIN;

UPDATE lcc_range SET concept_id=52443
WHERE concept_id=13506
  AND start_code='TS2284' AND end_code='TS2288';

UPDATE lcc_range SET concept_id=52489
WHERE concept_id=13508
  AND start_code='TS780' AND end_code='TS788';

INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal) VALUES
 (52443,361,1),
 (52489,361,1);

DELETE FROM lcc_range
WHERE concept_id=13539
  AND start_code='NA2440' AND end_code='NA2460';

COMMIT;

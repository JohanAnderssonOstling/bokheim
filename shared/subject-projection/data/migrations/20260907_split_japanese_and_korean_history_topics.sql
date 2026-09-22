BEGIN;

INSERT INTO concept(concept_id,preferred_label) VALUES
  (50863,'Japanese Civilization & Social Life'),
  (50864,'Japanese Ethnography'),
  (50865,'Japanese Historiography'),
  (50866,'Korean Ethnography'),
  (50867,'Korean Historiography');

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal) VALUES
  (50863,3498,0),
  (50864,3498,0),
  (50865,3498,0),
  (50866,3500,0),
  (50867,3500,0);

UPDATE lcc_range SET concept_id=50863
WHERE concept_id=3498 AND class_letters='DS'
  AND start_value='820.8' AND end_value='827';

UPDATE lcc_range SET concept_id=50864
WHERE concept_id=3498 AND class_letters='DS'
  AND start_value='830' AND end_value='832';

UPDATE lcc_range SET concept_id=50865
WHERE concept_id=3498 AND class_letters='DS'
  AND start_value='834.7' AND end_value='834.9';

UPDATE lcc_range SET concept_id=50866
WHERE concept_id=3500 AND class_letters='DS'
  AND start_value='904.5' AND end_value='904.7';

UPDATE lcc_range SET concept_id=50867
WHERE concept_id=3500 AND class_letters='DS'
  AND start_value='905.7' AND end_value='905.82';

UPDATE lcc_range SET concept_id=10686
WHERE concept_id=3500 AND class_letters='DS'
  AND ((start_value='909' AND end_value='909.2')
    OR (start_value='909.5' AND end_value='909.7'));

UPDATE lcc_range SET concept_id=10571
WHERE concept_id=3500 AND class_letters='DS'
  AND start_value='909.95' AND end_value='910';

COMMIT;

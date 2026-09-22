BEGIN;

INSERT INTO concept(concept_id,preferred_label) VALUES
  (50868,'Middle Eastern Local History & Places'),
  (50869,'Middle Eastern Ethnography'),
  (50870,'Middle Eastern Historiography'),
  (50871,'Brazilian Antiquities & Indigenous History'),
  (50872,'Brazilian Historiography'),
  (50873,'Brazilian Cities & Towns');

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal) VALUES
  (50868,3633,0),
  (50869,3633,0),
  (50870,3633,0),
  (50871,3427,0),
  (50872,3427,0),
  (50873,3427,0);

UPDATE lcc_range SET concept_id=50868
WHERE concept_id=3633 AND class_letters='DS'
  AND CAST(start_value AS REAL) >= 50.2
  AND CAST(end_value AS REAL) <= 54.95;

UPDATE lcc_range SET concept_id=50869
WHERE concept_id=3633 AND class_letters='DS'
  AND start_value='58' AND end_value='59';

UPDATE lcc_range SET concept_id=50870
WHERE concept_id=3633 AND class_letters='DS'
  AND CAST(start_value AS REAL) >= 61
  AND CAST(end_value AS REAL) <= 61.7;

UPDATE lcc_range SET concept_id=50871
WHERE concept_id=3427 AND class_letters='F'
  AND start_value='2518.5' AND end_value='2520.1';

UPDATE lcc_range SET concept_id=50872
WHERE concept_id=3427 AND class_letters='F'
  AND start_value='2520.4' AND end_value='2520.6';

UPDATE lcc_range SET concept_id=50873
WHERE concept_id=3427 AND class_letters='F'
  AND start_value='2646' AND end_value='2651';

COMMIT;

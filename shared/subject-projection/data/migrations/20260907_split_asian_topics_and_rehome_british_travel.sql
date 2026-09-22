BEGIN;

INSERT INTO concept(concept_id,preferred_label) VALUES
  (50853,'Asian Antiquities'),
  (50854,'Asian Ethnography');

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal) VALUES
  (50853,3485,0),
  (50854,3485,0);

-- Description and travel is not history.
UPDATE lcc_range SET concept_id=2433
WHERE concept_id=3485 AND class_letters='DS'
  AND start_value='5.95' AND end_value='10';

INSERT INTO lcc_range(class_letters,start_value,end_value,concept_id)
VALUES ('DS','11','11',50853);

UPDATE lcc_range SET concept_id=50854
WHERE concept_id=3485 AND class_letters='DS'
  AND CAST(start_value AS REAL) >= 13
  AND CAST(end_value AS REAL) <= 28;

-- Route England through the existing country-history node. Its chronological
-- and locality descendants remain more specific; guidebooks are excluded.
INSERT INTO lcc_range(class_letters,start_value,end_value,concept_id) VALUES
  ('DA','20','592',3566),
  ('DA','670','690',3566);

DELETE FROM lcc_range
WHERE concept_id=3521 AND class_letters='DA'
  AND (start_value,end_value) IN (
    VALUES ('498','503'),('505','522'),('550','565')
  );

INSERT INTO lcc_range(class_letters,start_value,end_value,concept_id)
VALUES ('DA','600','667',2461);

COMMIT;

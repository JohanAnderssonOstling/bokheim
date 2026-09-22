-- Replace the QL umbrella with complete, high-use official Zoology divisions.

INSERT INTO concept(concept_id,preferred_label)
VALUES
  (53313,'Zoology Reference & Institutions'),
  (53314,'Wildlife Conservation & Extinct Animals'),
  (53315,'Dangerous & Poisonous Animals'),
  (53316,'Animal Geography & Faunas'),
  (53317,'Zoological Classification & Nomenclature');

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal)
VALUES
  (53313,5048,0),
  (53314,5048,0),
  (53315,5048,0),
  (53316,5048,0),
  (53317,5048,0),
  (1138,5048,1);

DELETE FROM lcc_range
WHERE concept_id=5048
  AND start_code='QL1'
  AND end_code='QL991';

INSERT INTO lcc_range(start_code,end_code,concept_id)
VALUES
  ('QL1','QL75',53313),
  ('QL76','QL79',1138),
  ('QL81.5','QL89.2',53314),
  ('QL100','QL100.5',53315),
  ('QL101','QL345.32',53316),
  ('QL351','QL355',53317);

-- Remove ranges wholly contained by a broader range on the same destination.
DELETE FROM lcc_range
WHERE (concept_id,start_code,end_code) IN (
  (5049,'QL810','QL813'),
  (5049,'QL845','QL855'),
  (5050,'QL751.992','QL785.5'),
  (5052,'QL954','QL954.2'),
  (5052,'QL961','QL979.5'),
  (5053,'QL362.45','QL362.46'),
  (5053,'QL380','QL380.8'),
  (5053,'QL400','QL400.5')
);

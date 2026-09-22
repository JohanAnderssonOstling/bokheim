BEGIN;

-- High-volume History ranges whose LCC captions describe durable topical or
-- geographic subdivisions. Each structural interval keeps a single owner.
INSERT INTO concept(concept_id,preferred_label) VALUES
  (50834,'Chinese Antiquities'),
  (50835,'Chinese Civilization & Social Life'),
  (50836,'Chinese Ethnography'),
  (50837,'Manchurian History'),
  (50838,'Tibetan History'),
  (50839,'Chinese Cities & Local Regions'),
  (50840,'Jewish Diaspora'),
  (50841,'Mexican Antiquities & Indigenous History'),
  (50842,'Canadian Provinces & Territories'),
  (50843,'Polish Local History & Description');

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal) VALUES
  (50834,3493,0),
  (50835,3493,0),
  (50836,3493,0),
  (50837,3493,0),
  (50838,3493,0),
  (50839,3493,0),
  (50840,3638,0),
  (50841,3391,0),
  (50842,3400,0),
  (50843,3610,0);

UPDATE lcc_range SET concept_id=50834
WHERE concept_id=3493 AND class_letters='DS'
  AND start_value='714' AND end_value='719';

UPDATE lcc_range SET concept_id=50835
WHERE concept_id=3493 AND class_letters='DS'
  AND start_value='721' AND end_value='727';

UPDATE lcc_range SET concept_id=50836
WHERE concept_id=3493 AND class_letters='DS'
  AND start_value='730' AND end_value='731';

UPDATE lcc_range SET concept_id=50837
WHERE concept_id=3493 AND class_letters='DS'
  AND start_value='781' AND end_value='784.2';

UPDATE lcc_range SET concept_id=50838
WHERE concept_id=3493 AND class_letters='DS'
  AND start_value='785' AND end_value='786';

UPDATE lcc_range SET concept_id=50839
WHERE concept_id=3493 AND class_letters='DS'
  AND start_value IN ('795','795.0')
  AND CAST(end_value AS REAL) BETWEEN 795 AND 797.88;

-- DS781-796 is only an umbrella for Chinese local history. Retaining it on
-- the parent would compete with the mutually exclusive locality leaves.
DELETE FROM lcc_range
WHERE concept_id=3493 AND class_letters='DS'
  AND start_value='781' AND end_value='796';

UPDATE lcc_range SET concept_id=50840
WHERE concept_id=3638 AND class_letters='DS'
  AND start_value='133' AND end_value='151';

UPDATE lcc_range SET concept_id=50841
WHERE concept_id=3391 AND class_letters='F'
  AND CAST(start_value AS REAL) >= 1218.5
  AND CAST(end_value AS REAL) <= 1221;

UPDATE lcc_range SET concept_id=50842
WHERE concept_id=3400 AND class_letters='F'
  AND CAST(start_value AS REAL) >= 1035.8
  AND CAST(end_value AS REAL) <= 1145.2;

UPDATE lcc_range SET concept_id=50843
WHERE concept_id=3610 AND class_letters='DK'
  AND CAST(start_value AS REAL) >= 4600
  AND CAST(end_value AS REAL) <= 4800;

COMMIT;

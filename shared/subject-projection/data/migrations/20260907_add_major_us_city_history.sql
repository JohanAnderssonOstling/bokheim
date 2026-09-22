BEGIN;

INSERT INTO concept(concept_id,preferred_label) VALUES
  (50921,'New York City'),
  (50922,'Los Angeles'),
  (50923,'San Francisco'),
  (50924,'Austin'),
  (50925,'Dallas'),
  (50926,'Houston'),
  (50927,'San Antonio'),
  (50928,'Boston'),
  (50929,'Philadelphia'),
  (50930,'Pittsburgh');

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal) VALUES
  (50921,3461,0),
  (50922,3435,0),
  (50923,3435,0),
  (50924,3472,0),
  (50925,3472,0),
  (50926,3472,0),
  (50927,3472,0),
  (50928,3450,0),
  (50929,3467,0),
  (50930,3467,0);

-- Replace New York City's redundant nested ranges with one city span.
DELETE FROM lcc_range
WHERE concept_id=3461 AND class_letters='F'
  AND CAST(start_value AS REAL)>=128 AND CAST(end_value AS REAL)<=128.9;
INSERT INTO lcc_range VALUES('F','128','128.999',50921);

UPDATE lcc_range SET concept_id=50922
WHERE concept_id=3435 AND class_letters='F'
  AND start_value='869.L8' AND end_value='869.L89';
UPDATE lcc_range SET concept_id=50923
WHERE concept_id=3435 AND class_letters='F'
  AND start_value='869.S3' AND end_value='869.S39';

UPDATE lcc_range SET concept_id=50924
WHERE concept_id=3472 AND class_letters='F'
  AND start_value='394.A9' AND end_value='394.A99';
UPDATE lcc_range SET concept_id=50925
WHERE concept_id=3472 AND class_letters='F'
  AND start_value='394.D21' AND end_value='394.D219';
UPDATE lcc_range SET concept_id=50926
WHERE concept_id=3472 AND class_letters='F'
  AND start_value='394.H8' AND end_value='394.H89';
UPDATE lcc_range SET concept_id=50927
WHERE concept_id=3472 AND class_letters='F'
  AND start_value='394.S211' AND end_value='394.S2119';

-- Boston and Philadelphia had several overlapping period/population ranges.
-- Their consolidated city spans retain those subdivisions without duplicate
-- ownership inside the same city.
DELETE FROM lcc_range
WHERE concept_id=3450 AND class_letters='F'
  AND CAST(start_value AS REAL)>=73 AND CAST(end_value AS REAL)<74;
INSERT INTO lcc_range VALUES('F','73','73.999',50928);

DELETE FROM lcc_range
WHERE concept_id=3467 AND class_letters='F'
  AND CAST(start_value AS REAL)>=158 AND CAST(end_value AS REAL)<159;
INSERT INTO lcc_range VALUES('F','158','158.999',50929);

UPDATE lcc_range SET concept_id=50930
WHERE concept_id=3467 AND class_letters='F'
  AND start_value='159.P6' AND end_value='159.P69';

COMMIT;

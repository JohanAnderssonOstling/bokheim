BEGIN;

INSERT INTO concept(concept_id,preferred_label) VALUES
  (50855,'Jerusalem'),
  (50856,'Israelite & Palestinian Antiquities'),
  (50857,'Ethnography & Tribes of Israel'),
  (50858,'Iranian Antiquities'),
  (50859,'Iranian Ethnography'),
  (50860,'Iranian Local History'),
  (50861,'English Antiquities, Society & Ethnography'),
  (50862,'English Political & Military History');

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal) VALUES
  (50855,3638,0),
  (50856,3638,0),
  (50857,3638,0),
  (50858,3636,0),
  (50859,3636,0),
  (50860,3636,0),
  (50861,3566,0),
  (50862,3566,0);

UPDATE lcc_range SET concept_id=50855
WHERE concept_id=3638 AND class_letters='DS'
  AND CAST(start_value AS REAL) >= 109
  AND CAST(end_value AS REAL) <= 109.94;

UPDATE lcc_range SET concept_id=50856
WHERE concept_id=3638 AND class_letters='DS'
  AND CAST(start_value AS REAL) >= 111
  AND CAST(end_value AS REAL) <= 111.9;

UPDATE lcc_range SET concept_id=50857
WHERE concept_id=3638 AND class_letters='DS'
  AND CAST(start_value AS REAL) >= 113.2
  AND CAST(end_value AS REAL) <= 113.8;

UPDATE lcc_range SET concept_id=50858
WHERE concept_id=3636 AND class_letters='DS'
  AND CAST(start_value AS REAL) >= 260.7
  AND CAST(end_value AS REAL) <= 262;

UPDATE lcc_range SET concept_id=50859
WHERE concept_id=3636 AND class_letters='DS'
  AND CAST(start_value AS REAL) >= 268
  AND CAST(end_value AS REAL) <= 269;

UPDATE lcc_range SET concept_id=50860
WHERE concept_id=3636 AND class_letters='DS'
  AND CAST(start_value AS REAL) >= 324
  AND CAST(end_value AS REAL) <= 326;

INSERT INTO lcc_range(class_letters,start_value,end_value,concept_id) VALUES
  ('DA','90','125',50861),
  ('DA','40','89.6',50862);

INSERT OR IGNORE INTO source_selector(concept_id,system_id,selector) VALUES
  (13658,'lcc','DA670'),
  (13659,'lcc','DA690');

COMMIT;

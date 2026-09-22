BEGIN;

INSERT INTO concept(concept_id,preferred_label) VALUES
  (50844,'North Korean History'),
  (50845,'Korean Local History'),
  (50846,'Indian Ethnography & Communities'),
  (50847,'Indian Local History'),
  (50848,'Japanese Local History'),
  (50849,'Tribal Nations & Cultures'),
  (50850,'African American Status & Social Development'),
  (50851,'African American Biography & Genealogy'),
  (50852,'Falkland Islands History');

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal) VALUES
  (50844,3500,0),
  (50845,3500,0),
  (50846,3496,0),
  (50847,3496,0),
  (50848,3498,0),
  (50849,3398,0),
  (50850,3378,0),
  (50851,3378,0),
  (50852,3385,0);

-- Move broad regional schedules from Asian History to its already existing,
-- specifically named regional children. More specific country ranges still
-- win beneath these nodes.
INSERT OR IGNORE INTO lcc_range(class_letters,start_value,end_value,concept_id) VALUES
  ('DS','501','518.9',3494),
  ('DS','331','349.9',3508),
  ('DS','798.92','799.9',10699);

UPDATE lcc_range SET concept_id=11573
WHERE concept_id=3485 AND class_letters='DS'
  AND CAST(start_value AS REAL) >= 327
  AND CAST(end_value AS REAL) <= 329.8;

UPDATE lcc_range SET concept_id=3513
WHERE concept_id=3485 AND class_letters='DS'
  AND CAST(start_value AS REAL) >= 559.92
  AND CAST(end_value AS REAL) <= 559.93;

-- The preliminary China span belongs on the existing Chinese History node;
-- it is not turned into a synthetic “General” child.
INSERT INTO lcc_range(class_letters,start_value,end_value,concept_id)
VALUES ('DS','701','713.99',3493);

INSERT OR IGNORE INTO source_selector(concept_id,system_id,selector)
VALUES (10535,'lcc','DS798');

-- The E99 cutter sequence is explicitly Indian tribes and cultures.
INSERT INTO source_selector(concept_id,system_id,selector)
VALUES (50849,'lcc','E99');
INSERT INTO lcc_range(class_letters,start_value,end_value,concept_id)
VALUES ('E','99.A','99.Z',50849);

-- Specific Indian, Japanese, and Korean schedule divisions.
INSERT INTO lcc_range(class_letters,start_value,end_value,concept_id) VALUES
  ('DS','430','432',50846),
  ('DS','483','486.8',50847);

UPDATE lcc_range SET concept_id=50848
WHERE concept_id=3498 AND class_letters='DS'
  AND CAST(start_value AS REAL) >= 894.2
  AND CAST(end_value AS REAL) <= 897;
INSERT OR IGNORE INTO lcc_range(class_letters,start_value,end_value,concept_id)
VALUES ('DS','894.215','897',50848);

UPDATE lcc_range SET concept_id=50845
WHERE concept_id=3500 AND class_letters='DS'
  AND CAST(start_value AS REAL) >= 924
  AND CAST(end_value AS REAL) <= 925;

UPDATE lcc_range SET concept_id=50844
WHERE concept_id=3500 AND class_letters='DS'
  AND CAST(start_value AS REAL) >= 930
  AND CAST(end_value AS REAL) <= 937;

-- Durable topical divisions within African American history.
INSERT INTO lcc_range(class_letters,start_value,end_value,concept_id) VALUES
  ('E','185.2','185.89',50850),
  ('E','185.96','185.98',50851);

-- LCC separates the Falkland Islands from Argentina. Keep it as a sibling
-- under Latin American History rather than as an Argentine subdivision.
INSERT INTO lcc_range(class_letters,start_value,end_value,concept_id)
VALUES ('F','3031','3031.5',50852);

COMMIT;

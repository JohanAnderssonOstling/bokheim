BEGIN;

INSERT INTO concept(concept_id,preferred_label) VALUES
  (51177,'Southern Historiography'),
  (51178,'Southern Geography'),
  (51179,'Southern Historic Places'),
  (51180,'Southern Antiquities'),
  (51181,'Southern History, 1775–1865'),
  (51182,'Southern History, 1865–1950'),
  (51183,'Southern History Since 1951'),
  (51184,'Gulf States');

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal) VALUES
  (51177,3422,0),(51178,3422,0),(51179,3422,0),(51180,3422,0),
  (51181,3422,0),(51182,3422,0),(51183,3422,0),(51184,3422,0);

UPDATE lcc_selector SET concept_id=51177 WHERE concept_id=3422 AND selector='F208.2';
UPDATE lcc_selector SET concept_id=51178 WHERE concept_id=3422 AND selector='F209.8';
UPDATE lcc_selector SET concept_id=51179 WHERE concept_id=3422 AND selector='F210';
UPDATE lcc_selector SET concept_id=51180 WHERE concept_id=3422 AND selector IN ('F211','F212');
UPDATE lcc_selector SET concept_id=51181 WHERE concept_id=3422 AND selector='F213';
UPDATE lcc_selector SET concept_id=51184 WHERE concept_id=3422 AND selector='F296';

-- These selectors already have more specific geographic destinations.
UPDATE lcc_selector SET concept_id=3439 WHERE concept_id=3422 AND selector='F301';
UPDATE lcc_selector SET concept_id=50904 WHERE concept_id=3422 AND selector='F396';

DELETE FROM lcc_range
WHERE concept_id=3422 AND class_letters='F'
  AND start_value='215' AND end_value='216.2';

INSERT INTO lcc_range(class_letters,start_value,end_value,concept_id) VALUES
  ('F','215','216.199',51182),
  ('F','216.2','216.299',51183);

COMMIT;

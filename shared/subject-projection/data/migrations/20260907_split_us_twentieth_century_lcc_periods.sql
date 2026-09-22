BEGIN;

INSERT INTO concept(concept_id,preferred_label) VALUES
  (51008,'1901–1913'),
  (51009,'McKinley Administration, 1901'),
  (51010,'Theodore Roosevelt Administrations, 1901–1909'),
  (51011,'Taft Administration, 1909–1913'),
  (51012,'Wilson Administrations, 1913–1921'),
  (51013,'The Twenties, 1919–1933'),
  (51014,'Harding Administration, 1921–1923'),
  (51015,'Coolidge Administration, 1923–1929'),
  (51016,'Hoover Administration, 1929–1933'),
  (51017,'Franklin D. Roosevelt Administrations, 1933–1945'),
  (51018,'Truman Administrations, 1945–1953'),
  (51019,'Eisenhower Administrations, 1953–1961');

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal) VALUES
  (51008,3405,0),
  (51009,51008,0),(51010,51008,1),(51011,51008,2),
  (51012,3405,1),
  (51013,3405,2),
  (51014,51013,0),(51015,51013,1),(51016,51013,2),
  (51017,3405,3),(51018,3405,4),(51019,3405,5);

-- These boundaries reproduce the Library of Congress E-F schedule. General
-- twentieth-century material in E740-E749 deliberately remains on the parent.
INSERT INTO lcc_range(class_letters,start_value,end_value,concept_id) VALUES
  ('E','751','765.999',51008),
  ('E','791','796',51015);

UPDATE lcc_range SET concept_id=51009 WHERE concept_id=3405 AND class_letters='E' AND start_value='751' AND end_value='751.2';
UPDATE lcc_range SET concept_id=51010 WHERE concept_id=3405 AND class_letters='E' AND start_value='756' AND end_value='760';
UPDATE lcc_range SET concept_id=51011 WHERE concept_id=3405 AND class_letters='E' AND start_value='761' AND end_value='765';
UPDATE lcc_range SET concept_id=51012 WHERE concept_id=3405 AND class_letters='E' AND start_value='766' AND end_value='783';
UPDATE lcc_range SET concept_id=51013 WHERE concept_id=3405 AND class_letters='E' AND start_value='784' AND end_value='805';
UPDATE lcc_range SET concept_id=51014 WHERE concept_id=3405 AND class_letters='E' AND start_value='785' AND end_value='786.2';
UPDATE lcc_range SET concept_id=51016 WHERE concept_id=3405 AND class_letters='E' AND start_value='801' AND end_value='805';
UPDATE lcc_range SET concept_id=51017 WHERE concept_id=3405 AND class_letters='E' AND start_value='806' AND end_value='812';
UPDATE lcc_range SET concept_id=51018 WHERE concept_id=3405 AND class_letters='E' AND start_value='813' AND end_value='816';
UPDATE lcc_range SET concept_id=51019 WHERE concept_id=3405 AND class_letters='E' AND start_value='835' AND end_value='837.7';

COMMIT;

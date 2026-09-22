BEGIN;

INSERT INTO concept(concept_id,preferred_label) VALUES
  (51144,'U.S. Biography, 1865–1900'),
  (51145,'Andrew Johnson Administration, 1865–1869'),
  (51146,'Grant Administrations, 1869–1877'),
  (51147,'Hayes Administration, 1877–1881'),
  (51148,'Garfield Administration, 1881'),
  (51149,'Arthur Administration, 1881–1885'),
  (51150,'Cleveland Administration, 1885–1889'),
  (51151,'Benjamin Harrison Administration, 1889–1893'),
  (51152,'Cleveland Administration, 1893–1897'),
  (51153,'McKinley Administration, 1897–1901'),
  (51154,'Spanish–American War');

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal) VALUES
  (51144,3412,0),(51144,51004,1),
  (51145,3412,0),(51146,3412,0),(51147,3412,0),
  (51148,3412,0),(51149,3412,0),(51150,3412,0),
  (51151,3412,0),(51152,3412,0),(51153,3412,0),
  (51154,3412,0),(51154,51003,1);

-- Replace all overlapping period umbrellas with mutually exclusive official
-- LCC sections. General period material stays on the parent. Campaign numbers
-- E670, E675, E680, E685, E695, E700, E705, E710, and E738 are not assigned.
-- The Spanish-American War remains a broad leaf: its battles and campaigns are
-- not expanded into taxonomy subjects.
DELETE FROM lcc_range WHERE concept_id=3412 AND class_letters='E';

INSERT INTO lcc_range(class_letters,start_value,end_value,concept_id) VALUES
  ('E','660','662.999',3412),
  ('E','663','664.999',51144),
  ('E','666','669.999',51145),
  ('E','671','674.999',51146),
  ('E','676','679.999',51146),
  ('E','681','684.999',51147),
  ('E','686','687.999',51148),
  ('E','691','694.999',51149),
  ('E','696','699.999',51150),
  ('E','701','704.999',51151),
  ('E','706','709.999',51152),
  ('E','711','713.999',51153),
  ('E','714','735.999',51154);

-- E310.7 is Constitutional-period diplomatic history and is already covered
-- by the existing Constitutional Era range.
DELETE FROM lcc_selector WHERE concept_id=3412 AND selector='E310.7';

COMMIT;

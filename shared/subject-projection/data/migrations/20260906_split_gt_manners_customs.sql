BEGIN IMMEDIATE;

UPDATE concept SET preferred_label = 'Manners, Customs & Material Culture'
WHERE concept_id = 'unified:f01babe0-0d59-505b-8c82-1c03d6e7a145';

DELETE FROM source_selector
WHERE concept_id = 'unified:f01babe0-0d59-505b-8c82-1c03d6e7a145';

INSERT OR IGNORE INTO concept (concept_id, preferred_label) VALUES
  ('bokheim:customs:dwellings', 'Houses & Dwellings'),
  ('bokheim:customs:churchgoing', 'Churchgoing Customs'),
  ('bokheim:customs:private-life', 'Private-Life Customs'),
  ('bokheim:customs:public-social-life', 'Public & Social Customs'),
  ('bokheim:customs:travel', 'Travel & Transportation Customs'),
  ('bokheim:customs:status-groups', 'Customs by Birth, Rank & Status'),
  ('bokheim:customs:occupations', 'Occupational Customs'),
  ('bokheim:customs:special-groups', 'Other Social-Group Customs');

INSERT OR IGNORE INTO concept_parent
  (concept_id, parent_concept_id, ordinal, display_order) VALUES
  ('unified:e10ba084-d045-5872-bc60-ad7829a7e19b', 'unified:f01babe0-0d59-505b-8c82-1c03d6e7a145', 1, 10),
  ('bokheim:customs:dwellings', 'unified:f01babe0-0d59-505b-8c82-1c03d6e7a145', 0, 20),
  ('bokheim:customs:churchgoing', 'unified:f01babe0-0d59-505b-8c82-1c03d6e7a145', 0, 30),
  ('unified:92f60b54-b1db-55f9-bcbd-5d6dde135175', 'unified:f01babe0-0d59-505b-8c82-1c03d6e7a145', 1, 40),
  ('unified:f6da22bc-4f79-59e0-88de-3d63d94a0210', 'unified:f01babe0-0d59-505b-8c82-1c03d6e7a145', 1, 50),
  ('bokheim:customs:private-life', 'unified:f01babe0-0d59-505b-8c82-1c03d6e7a145', 0, 60),
  ('bokheim:customs:public-social-life', 'unified:f01babe0-0d59-505b-8c82-1c03d6e7a145', 0, 70),
  ('bokheim:customs:travel', 'unified:f01babe0-0d59-505b-8c82-1c03d6e7a145', 0, 80),
  ('bokheim:customs:status-groups', 'unified:f01babe0-0d59-505b-8c82-1c03d6e7a145', 0, 90),
  ('bokheim:customs:occupations', 'unified:f01babe0-0d59-505b-8c82-1c03d6e7a145', 0, 100),
  ('bokheim:customs:special-groups', 'unified:f01babe0-0d59-505b-8c82-1c03d6e7a145', 0, 110);

INSERT INTO source_selector (concept_id, system_id, selector, ordinal) VALUES
  ('unified:e10ba084-d045-5872-bc60-ad7829a7e19b', 'lcc', 'GT1..GT164.9@1', 0),
  ('unified:e10ba084-d045-5872-bc60-ad7829a7e19b', 'lcc', 'GT476.1..GT484.9@1', 1),
  ('unified:e10ba084-d045-5872-bc60-ad7829a7e19b', 'lcc', 'GT485.1..GT494.9@1', 2),
  ('unified:e10ba084-d045-5872-bc60-ad7829a7e19b', 'lcc', 'GT499.1..GT499.9@1', 3),
  ('unified:e10ba084-d045-5872-bc60-ad7829a7e19b', 'lcc', 'GT2370.1..GT2399.9@1', 4),
  ('unified:e10ba084-d045-5872-bc60-ad7829a7e19b', 'lcc', 'GT3390.6..GT3399.9@1', 5),
  ('unified:e10ba084-d045-5872-bc60-ad7829a7e19b', 'lcc', 'GT5090.1..GT5219.9@1', 6),
  ('unified:e10ba084-d045-5872-bc60-ad7829a7e19b', 'lcc', 'GT5286.1..GT5319.9@1', 7),
  ('unified:e10ba084-d045-5872-bc60-ad7829a7e19b', 'lcc', 'GT6737.1..GT7070@1', 8),
  ('bokheim:customs:dwellings', 'lcc', 'GT165..GT476@1', 0),
  ('bokheim:customs:churchgoing', 'lcc', 'GT485', 0),
  ('unified:92f60b54-b1db-55f9-bcbd-5d6dde135175', 'lcc', 'GT495..GT499@1', 0),
  ('unified:f6da22bc-4f79-59e0-88de-3d63d94a0210', 'lcc', 'GT500..GT2370@1', 0),
  ('bokheim:customs:private-life', 'lcc', 'GT2400..GT3390.5@1', 0),
  ('bokheim:customs:public-social-life', 'lcc', 'GT3400..GT5090@1', 0),
  ('bokheim:customs:travel', 'lcc', 'GT5220..GT5286@1', 0),
  ('bokheim:customs:status-groups', 'lcc', 'GT5320..GT5690@1', 0),
  ('bokheim:customs:occupations', 'lcc', 'GT5750..GT6390@1', 0),
  ('bokheim:customs:special-groups', 'lcc', 'GT5690.1..GT5749.9@1', 0),
  ('bokheim:customs:special-groups', 'lcc', 'GT6390.1..GT6737@1', 1);

COMMIT;

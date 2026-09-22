BEGIN;

INSERT INTO concept(concept_id,preferred_label) VALUES
  (50990,'1961–1969'),
  (50991,'1969–1981'),
  (50992,'1981–2000'),
  (50993,'Kennedy Administration, 1961–1963'),
  (50994,'Johnson Administration, 1963–1969'),
  (50995,'Nixon Administration, 1969–1974'),
  (50996,'Ford Administration, 1974–1977'),
  (50997,'Carter Administration, 1977–1981'),
  (50998,'Reagan Administration, 1981–1989'),
  (50999,'George H. W. Bush Administration, 1989–1993'),
  (51000,'Clinton Administration, 1993–2001');

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal) VALUES
  (50990,3413,0),(50991,3413,1),(50992,3413,2),
  (50993,50990,0),(50994,50990,1),
  (50995,50991,0),(50996,50991,1),(50997,50991,2),
  (50998,50992,0),(50999,50992,1),(51000,50992,2);

-- Broad LCC chronological blocks. Codes in the gaps between presidential
-- schedules remain on these period nodes.
INSERT INTO lcc_range(class_letters,start_value,end_value,concept_id) VALUES
  ('E','838','851.999',50990),
  ('E','852','875.999',50991),
  ('E','876','889.999',50992);

-- Official LCC presidential-administration schedules.
INSERT INTO lcc_range(class_letters,start_value,end_value,concept_id) VALUES
  ('E','841','843.999',50993),
  ('E','846','851.999',50994),
  ('E','855','861.999',50995),
  ('E','865','868.999',50996),
  ('E','872','875.999',50997),
  ('E','876','880.999',50998),
  ('E','881','884.999',50999),
  ('E','885','889.999',51000);

COMMIT;

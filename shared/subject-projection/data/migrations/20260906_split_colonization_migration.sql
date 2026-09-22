BEGIN IMMEDIATE;

DELETE FROM source_selector
WHERE concept_id = 'unified:25fa6bbd-9eba-5ab5-9220-b2fb9a950e7b';

-- Preserve the exact class notation on the parent; broad matching is confined
-- to the deliberately pruned leaves.
INSERT INTO source_selector (concept_id, system_id, selector, ordinal) VALUES
  ('unified:25fa6bbd-9eba-5ab5-9220-b2fb9a950e7b', 'lcc', 'JV', 0);

INSERT OR IGNORE INTO concept (concept_id, preferred_label) VALUES
  ('bokheim:colonization-migration:colonialism-general', 'Colonialism & Colonial Administration'),
  ('bokheim:colonization-migration:colonizing-nations', 'Colonization by Colonial Power'),
  ('bokheim:colonization-migration:migration-general', 'Migration: General Works & Policy'),
  ('bokheim:colonization-migration:emigration', 'Emigration'),
  ('bokheim:colonization-migration:immigration', 'Immigration'),
  ('bokheim:colonization-migration:americas', 'Migration in the Americas'),
  ('bokheim:colonization-migration:europe', 'Migration in Europe'),
  ('bokheim:colonization-migration:asia', 'Migration in Asia'),
  ('bokheim:colonization-migration:africa', 'Migration in Africa'),
  ('bokheim:colonization-migration:atlantic-indian-islands', 'Migration in Atlantic & Indian Ocean Islands'),
  ('bokheim:colonization-migration:australia-pacific', 'Migration in Australia & the Pacific'),
  ('bokheim:colonization-migration:polar-developing', 'Migration in Polar Regions & Developing Countries');

INSERT OR IGNORE INTO concept_parent
  (concept_id, parent_concept_id, ordinal, display_order) VALUES
  ('bokheim:colonization-migration:colonialism-general', 'unified:25fa6bbd-9eba-5ab5-9220-b2fb9a950e7b', 0, 10),
  ('bokheim:colonization-migration:colonizing-nations', 'unified:25fa6bbd-9eba-5ab5-9220-b2fb9a950e7b', 0, 20),
  ('bokheim:colonization-migration:migration-general', 'unified:25fa6bbd-9eba-5ab5-9220-b2fb9a950e7b', 0, 30),
  ('bokheim:colonization-migration:emigration', 'unified:25fa6bbd-9eba-5ab5-9220-b2fb9a950e7b', 0, 40),
  ('bokheim:colonization-migration:immigration', 'unified:25fa6bbd-9eba-5ab5-9220-b2fb9a950e7b', 0, 50),
  ('bokheim:colonization-migration:americas', 'unified:25fa6bbd-9eba-5ab5-9220-b2fb9a950e7b', 0, 60),
  ('bokheim:colonization-migration:europe', 'unified:25fa6bbd-9eba-5ab5-9220-b2fb9a950e7b', 0, 70),
  ('bokheim:colonization-migration:asia', 'unified:25fa6bbd-9eba-5ab5-9220-b2fb9a950e7b', 0, 80),
  ('bokheim:colonization-migration:africa', 'unified:25fa6bbd-9eba-5ab5-9220-b2fb9a950e7b', 0, 90),
  ('bokheim:colonization-migration:atlantic-indian-islands', 'unified:25fa6bbd-9eba-5ab5-9220-b2fb9a950e7b', 0, 100),
  ('bokheim:colonization-migration:australia-pacific', 'unified:25fa6bbd-9eba-5ab5-9220-b2fb9a950e7b', 0, 110),
  ('bokheim:colonization-migration:polar-developing', 'unified:25fa6bbd-9eba-5ab5-9220-b2fb9a950e7b', 0, 120);

INSERT INTO source_selector (concept_id, system_id, selector, ordinal) VALUES
  ('bokheim:colonization-migration:colonialism-general', 'lcc', 'JV1..JV499.9@1', 0),
  ('bokheim:colonization-migration:colonizing-nations', 'lcc', 'JV500..JV5397.Z@1', 0),
  ('bokheim:colonization-migration:migration-general', 'lcc', 'JV6001..JV6060.9@1', 0),
  ('bokheim:colonization-migration:emigration', 'lcc', 'JV6061..JV6197.9@1', 0),
  ('bokheim:colonization-migration:immigration', 'lcc', 'JV6198..JV6349.9@1', 0),
  ('bokheim:colonization-migration:americas', 'lcc', 'JV6350..JV7589.9@1', 0),
  ('bokheim:colonization-migration:europe', 'lcc', 'JV7590..JV8489.9@1', 0),
  ('bokheim:colonization-migration:asia', 'lcc', 'JV8490..JV8789.9@1', 0),
  ('bokheim:colonization-migration:africa', 'lcc', 'JV8790..JV9028.9@1', 0),
  ('bokheim:colonization-migration:atlantic-indian-islands', 'lcc', 'JV9029..JV9099.9@1', 0),
  ('bokheim:colonization-migration:australia-pacific', 'lcc', 'JV9100..JV9471.9@1', 0),
  ('bokheim:colonization-migration:polar-developing', 'lcc', 'JV9472..JV9480.Z@1', 0);

COMMIT;

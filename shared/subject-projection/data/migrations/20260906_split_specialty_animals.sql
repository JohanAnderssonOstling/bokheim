BEGIN IMMEDIATE;

DELETE FROM source_selector
WHERE concept_id = 'bokheim:animal-culture:specialty-animals';

INSERT OR IGNORE INTO concept (concept_id, preferred_label) VALUES
  ('bokheim:animal-culture:acclimatization', 'Animal Acclimatization'),
  ('bokheim:animal-culture:fur-animals', 'Fur-Bearing Animals'),
  ('bokheim:animal-culture:laboratory-animals', 'Laboratory Animals'),
  ('bokheim:animal-culture:zoo-animals', 'Zoo & Captive Wild Animals'),
  ('bokheim:animal-culture:small-animals', 'Small-Animal Care'),
  ('bokheim:animal-culture:pets-general', 'Pets: General'),
  ('bokheim:animal-culture:dogs', 'Dogs'),
  ('bokheim:animal-culture:cats', 'Cats'),
  ('bokheim:animal-culture:rabbits-hares', 'Rabbits & Hares'),
  ('bokheim:animal-culture:aquarium-animals', 'Fish & Aquarium Animals'),
  ('bokheim:animal-culture:other-pets', 'Other Pets'),
  ('bokheim:animal-culture:reptiles', 'Reptiles');

INSERT OR IGNORE INTO concept_parent
  (concept_id, parent_concept_id, ordinal, display_order) VALUES
  ('bokheim:animal-culture:acclimatization', 'bokheim:animal-culture:specialty-animals', 0, 10),
  ('bokheim:animal-culture:fur-animals', 'bokheim:animal-culture:specialty-animals', 0, 20),
  ('bokheim:animal-culture:laboratory-animals', 'bokheim:animal-culture:specialty-animals', 0, 30),
  ('bokheim:animal-culture:zoo-animals', 'bokheim:animal-culture:specialty-animals', 0, 40),
  ('bokheim:animal-culture:small-animals', 'bokheim:animal-culture:specialty-animals', 0, 50),
  ('bokheim:animal-culture:pets-general', 'bokheim:animal-culture:specialty-animals', 0, 60),
  ('bokheim:animal-culture:dogs', 'bokheim:animal-culture:specialty-animals', 0, 70),
  ('bokheim:animal-culture:cats', 'bokheim:animal-culture:specialty-animals', 0, 80),
  ('bokheim:animal-culture:rabbits-hares', 'bokheim:animal-culture:specialty-animals', 0, 90),
  ('bokheim:animal-culture:aquarium-animals', 'bokheim:animal-culture:specialty-animals', 0, 100),
  ('bokheim:animal-culture:other-pets', 'bokheim:animal-culture:specialty-animals', 0, 110),
  ('bokheim:animal-culture:reptiles', 'bokheim:animal-culture:specialty-animals', 0, 120);

INSERT INTO source_selector (concept_id, system_id, selector, ordinal) VALUES
  ('bokheim:animal-culture:acclimatization', 'lcc', 'SF87', 0),
  ('bokheim:animal-culture:fur-animals', 'lcc', 'SF402..SF405.4@1', 0),
  ('bokheim:animal-culture:laboratory-animals', 'lcc', 'SF405.5..SF407.Z@1', 0),
  ('bokheim:animal-culture:zoo-animals', 'lcc', 'SF408..SF408.9@1', 0),
  ('bokheim:animal-culture:small-animals', 'lcc', 'SF409..SF410.9@1', 0),
  ('bokheim:animal-culture:pets-general', 'lcc', 'SF411..SF420.9@1', 0),
  ('bokheim:animal-culture:dogs', 'lcc', 'SF421..SF440.2.Z@1', 0),
  ('bokheim:animal-culture:cats', 'lcc', 'SF441..SF450.Z@1', 0),
  ('bokheim:animal-culture:rabbits-hares', 'lcc', 'SF451..SF455.Z@1', 0),
  ('bokheim:animal-culture:aquarium-animals', 'lcc', 'SF456..SF458.83.Z@1', 0),
  ('bokheim:animal-culture:other-pets', 'lcc', 'SF458.84..SF459.Z@1', 0),
  ('bokheim:animal-culture:reptiles', 'lcc', 'SF515..SF515.5.Z@1', 0);

COMMIT;

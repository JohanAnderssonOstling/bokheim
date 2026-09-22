BEGIN IMMEDIATE;

UPDATE concept SET preferred_label = 'Horticulture & Garden Design'
WHERE concept_id = 'bokheim:subject:plant-culture:horticulture';
DELETE FROM source_selector
WHERE concept_id = 'bokheim:subject:plant-culture:horticulture';

-- Remove the duplicate all-Gardening BISAC leaf after redistributing its codes.
DELETE FROM source_selector
WHERE concept_id = 'unified:ad57ba81-11cd-52c4-af21-2f8e7a57d532';
DELETE FROM concept_parent
WHERE concept_id = 'unified:ad57ba81-11cd-52c4-af21-2f8e7a57d532';
DELETE FROM source_label
WHERE concept_id = 'unified:ad57ba81-11cd-52c4-af21-2f8e7a57d532';
DELETE FROM concept
WHERE concept_id = 'unified:ad57ba81-11cd-52c4-af21-2f8e7a57d532';

INSERT OR IGNORE INTO concept (concept_id, preferred_label) VALUES
  ('bokheim:horticulture:flowers-ornamentals', 'Flowers & Ornamental Plants'),
  ('bokheim:horticulture:gardens', 'Gardens & Gardening'),
  ('bokheim:horticulture:parks', 'Parks & Public Gardens');

INSERT OR IGNORE INTO concept_parent
  (concept_id, parent_concept_id, ordinal, display_order) VALUES
  ('unified:255366d5-8a6f-51fd-b9c1-0b24335d1abd', 'bokheim:subject:plant-culture:horticulture', 1, 10),
  ('bokheim:horticulture:flowers-ornamentals', 'bokheim:subject:plant-culture:horticulture', 0, 20),
  ('bokheim:horticulture:gardens', 'bokheim:subject:plant-culture:horticulture', 0, 30),
  ('unified:45b26c33-3145-54e8-aad4-2106c1bb14f7', 'bokheim:subject:plant-culture:horticulture', 1, 40),
  ('bokheim:horticulture:parks', 'bokheim:subject:plant-culture:horticulture', 0, 50);

INSERT INTO source_selector (concept_id, system_id, selector, ordinal) VALUES
  ('unified:255366d5-8a6f-51fd-b9c1-0b24335d1abd', 'bisac', 'GAR000000', 1),
  ('unified:255366d5-8a6f-51fd-b9c1-0b24335d1abd', 'bisac', 'GAR002000', 2),
  ('unified:255366d5-8a6f-51fd-b9c1-0b24335d1abd', 'bisac', 'GAR018000', 3),
  ('unified:255366d5-8a6f-51fd-b9c1-0b24335d1abd', 'bisac', 'GAR019000', 4),
  ('unified:255366d5-8a6f-51fd-b9c1-0b24335d1abd', 'bisac', 'GAR019010', 5),
  ('unified:255366d5-8a6f-51fd-b9c1-0b24335d1abd', 'bisac', 'GAR019020', 6),
  ('unified:255366d5-8a6f-51fd-b9c1-0b24335d1abd', 'bisac', 'GAR019030', 7),
  ('unified:255366d5-8a6f-51fd-b9c1-0b24335d1abd', 'bisac', 'GAR019040', 8),
  ('unified:255366d5-8a6f-51fd-b9c1-0b24335d1abd', 'bisac', 'GAR019050', 9),
  ('unified:255366d5-8a6f-51fd-b9c1-0b24335d1abd', 'bisac', 'GAR019060', 10),
  ('unified:255366d5-8a6f-51fd-b9c1-0b24335d1abd', 'bisac', 'GAR019070', 11),
  ('unified:255366d5-8a6f-51fd-b9c1-0b24335d1abd', 'bisac', 'GAR019080', 12),
  ('unified:255366d5-8a6f-51fd-b9c1-0b24335d1abd', 'bisac', 'GAR030000', 13),
  ('unified:255366d5-8a6f-51fd-b9c1-0b24335d1abd', 'lcc', 'SB317.5..SB319.864@1', 0),

  ('bokheim:horticulture:flowers-ornamentals', 'bisac', 'GAR004000', 0),
  ('bokheim:horticulture:flowers-ornamentals', 'bisac', 'GAR004010', 1),
  ('bokheim:horticulture:flowers-ornamentals', 'bisac', 'GAR004030', 2),
  ('bokheim:horticulture:flowers-ornamentals', 'bisac', 'GAR004040', 3),
  ('bokheim:horticulture:flowers-ornamentals', 'bisac', 'GAR004050', 4),
  ('bokheim:horticulture:flowers-ornamentals', 'bisac', 'GAR004060', 5),
  ('bokheim:horticulture:flowers-ornamentals', 'bisac', 'GAR004080', 6),
  ('bokheim:horticulture:flowers-ornamentals', 'bisac', 'GAR017000', 7),
  ('bokheim:horticulture:flowers-ornamentals', 'bisac', 'GAR021000', 8),
  ('bokheim:horticulture:flowers-ornamentals', 'bisac', 'GAR023000', 9),
  ('bokheim:horticulture:flowers-ornamentals', 'lcc', 'SB403..SB450.87@1', 0),

  ('bokheim:horticulture:gardens', 'bisac', 'GAR001000', 0),
  ('bokheim:horticulture:gardens', 'bisac', 'GAR006000', 1),
  ('bokheim:horticulture:gardens', 'bisac', 'GAR007000', 2),
  ('bokheim:horticulture:gardens', 'bisac', 'GAR010000', 3),
  ('bokheim:horticulture:gardens', 'bisac', 'GAR013000', 4),
  ('bokheim:horticulture:gardens', 'bisac', 'GAR015000', 5),
  ('bokheim:horticulture:gardens', 'bisac', 'GAR020000', 6),
  ('bokheim:horticulture:gardens', 'bisac', 'GAR028000', 7),
  ('bokheim:horticulture:gardens', 'bisac', 'GAR029000', 8),
  ('bokheim:horticulture:gardens', 'lcc', 'SB450.9..SB467.8@1', 0),

  ('unified:45b26c33-3145-54e8-aad4-2106c1bb14f7', 'bisac', 'GAR014000', 1),
  ('unified:45b26c33-3145-54e8-aad4-2106c1bb14f7', 'lcc', 'SB469..SB476.4@1', 0),
  ('bokheim:horticulture:parks', 'lcc', 'SB481..SB486@1', 0),

  ('bokheim:subject:plant-culture:crops', 'bisac', 'GAR005000', 3),
  ('bokheim:subject:plant-culture:crops', 'bisac', 'GAR009000', 4),
  ('bokheim:subject:plant-culture:crops', 'bisac', 'GAR024000', 5),
  ('bokheim:subject:plant-culture:crops', 'bisac', 'GAR025000', 6),
  ('bokheim:subject:plant-culture:crops', 'bisac', 'GAR031000', 7),
  ('bokheim:subject:plant-culture:crops', 'bisac', 'TEC003110', 8),

  ('bokheim:subject:plant-culture:methods', 'bisac', 'GAR027000', 1),
  ('bokheim:subject:plant-culture:methods', 'bisac', 'GAR027010', 2),
  ('bokheim:subject:plant-culture:methods', 'bisac', 'GAR027020', 3),
  ('bokheim:subject:plant-culture:methods', 'bisac', 'GAR027030', 4),
  ('bokheim:subject:plant-culture:methods', 'bisac', 'GAR008000', 5),
  ('bokheim:subject:plant-culture:methods', 'bisac', 'GAR011000', 6),
  ('bokheim:subject:plant-culture:methods', 'bisac', 'GAR016000', 7),
  ('bokheim:subject:plant-culture:methods', 'bisac', 'GAR022000', 8);

COMMIT;

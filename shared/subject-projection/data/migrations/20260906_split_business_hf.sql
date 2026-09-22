BEGIN IMMEDIATE;

-- Business becomes a container. Its generated outline selectors duplicated
-- the more specific curated business concepts and caused all HF5001-HF6182
-- material to appear as a direct hit.
DELETE FROM source_selector
WHERE concept_id = 'unified:df4df9ce-4492-500d-a82d-1ce50c92f18b';

INSERT OR IGNORE INTO concept (concept_id, preferred_label) VALUES
  ('bokheim:business:general-reference', 'Business — General & Reference'),
  ('bokheim:business:careers', 'Careers & Professional Development'),
  ('bokheim:business:etiquette-broad', 'Business Etiquette'),
  ('bokheim:business:wholesale', 'Wholesale Trade'),
  ('bokheim:business:retail', 'Retail Trade'),
  ('bokheim:business:markets', 'Markets & Fairs'),
  ('bokheim:business:warehousing', 'Warehousing & Storage'),
  ('bokheim:business:organization', 'Business Organization & Administration'),
  ('bokheim:business:industrial-psychology', 'Industrial Psychology'),
  ('bokheim:business:personnel', 'Personnel Management'),
  ('bokheim:business:records', 'Business Records Management'),
  ('bokheim:business:shipping', 'Shipping & Delivery');

INSERT OR IGNORE INTO concept (concept_id, preferred_label) VALUES
  ('bokheim:business:other-activities-broad', 'Other Business Activities'),
  ('bokheim:business:special-lines-broad', 'Special Lines of Business');

-- Reuse existing BISAC concepts for cross-scheme unification.
INSERT OR IGNORE INTO concept_parent
  (concept_id, parent_concept_id, ordinal, display_order) VALUES
  ('bokheim:business:general-reference', 'unified:df4df9ce-4492-500d-a82d-1ce50c92f18b', 0, 10),
  ('bokheim:business:careers', 'unified:df4df9ce-4492-500d-a82d-1ce50c92f18b', 0, 20),
  ('unified:c16fe56b-50ca-5c65-b34e-577d9805d17d', 'unified:df4df9ce-4492-500d-a82d-1ce50c92f18b', 1, 30),
  ('bokheim:business:etiquette-broad', 'unified:df4df9ce-4492-500d-a82d-1ce50c92f18b', 0, 35),
  ('unified:54299b57-991f-5cd6-9dbd-192916fefc07', 'unified:df4df9ce-4492-500d-a82d-1ce50c92f18b', 1, 40),
  ('bokheim:business:wholesale', 'unified:df4df9ce-4492-500d-a82d-1ce50c92f18b', 0, 50),
  ('bokheim:business:retail', 'unified:df4df9ce-4492-500d-a82d-1ce50c92f18b', 0, 60),
  ('unified:86a67223-84fa-5f02-b8bd-ec6404fde740', 'unified:df4df9ce-4492-500d-a82d-1ce50c92f18b', 1, 70),
  ('unified:bf6d9d7e-b58f-5a84-97de-919f2ea27c43', 'unified:df4df9ce-4492-500d-a82d-1ce50c92f18b', 1, 80),
  ('bokheim:business:markets', 'unified:df4df9ce-4492-500d-a82d-1ce50c92f18b', 0, 90),
  ('bokheim:business:warehousing', 'unified:df4df9ce-4492-500d-a82d-1ce50c92f18b', 0, 100),
  ('bokheim:business:organization', 'unified:df4df9ce-4492-500d-a82d-1ce50c92f18b', 0, 105),
  ('unified:38d91f55-9a6a-5498-ade0-174fa32f3958', 'unified:df4df9ce-4492-500d-a82d-1ce50c92f18b', 1, 110),
  ('unified:1f945408-5a18-50fb-959d-3848b4b61c60', 'unified:df4df9ce-4492-500d-a82d-1ce50c92f18b', 1, 120),
  ('bokheim:business:industrial-psychology', 'unified:df4df9ce-4492-500d-a82d-1ce50c92f18b', 0, 130),
  ('bokheim:business:personnel', 'unified:df4df9ce-4492-500d-a82d-1ce50c92f18b', 0, 140),
  ('unified:66cf212d-88c2-5e3a-9cd3-7b4d76217132', 'unified:df4df9ce-4492-500d-a82d-1ce50c92f18b', 1, 150),
  ('unified:1055eec5-b7c3-519d-85d3-420d9ec68986', 'unified:df4df9ce-4492-500d-a82d-1ce50c92f18b', 1, 160),
  ('unified:b68e0249-6e60-59ca-bcfd-7caa71559ad2', 'unified:df4df9ce-4492-500d-a82d-1ce50c92f18b', 1, 170),
  ('unified:a7dc9eb0-abbb-5fdf-b366-c9d7c12580df', 'unified:df4df9ce-4492-500d-a82d-1ce50c92f18b', 1, 180),
  ('bokheim:business:records', 'unified:df4df9ce-4492-500d-a82d-1ce50c92f18b', 0, 190),
  ('bokheim:business:shipping', 'unified:df4df9ce-4492-500d-a82d-1ce50c92f18b', 0, 200),
  ('unified:2adce3e6-bc0f-5d1d-b75f-bc0215566401', 'unified:df4df9ce-4492-500d-a82d-1ce50c92f18b', 1, 210),
  ('bokheim:business:other-activities-broad', 'unified:df4df9ce-4492-500d-a82d-1ce50c92f18b', 0, 220),
  ('bokheim:business:special-lines-broad', 'unified:df4df9ce-4492-500d-a82d-1ce50c92f18b', 0, 230);

-- Normalize existing LCC ownership before inserting mutually exclusive spans.
DELETE FROM source_selector
WHERE system_id = 'lcc'
  AND selector GLOB 'HF*'
  AND concept_id IN (
  'unified:c16fe56b-50ca-5c65-b34e-577d9805d17d',
  'unified:54299b57-991f-5cd6-9dbd-192916fefc07',
  'unified:86a67223-84fa-5f02-b8bd-ec6404fde740',
  'unified:bf6d9d7e-b58f-5a84-97de-919f2ea27c43',
  'unified:38d91f55-9a6a-5498-ade0-174fa32f3958',
  'unified:1f945408-5a18-50fb-959d-3848b4b61c60',
  'unified:66cf212d-88c2-5e3a-9cd3-7b4d76217132',
  'unified:1055eec5-b7c3-519d-85d3-420d9ec68986',
  'unified:b68e0249-6e60-59ca-bcfd-7caa71559ad2',
  'unified:a7dc9eb0-abbb-5fdf-b366-c9d7c12580df',
  'unified:2adce3e6-bc0f-5d1d-b75f-bc0215566401'
);

INSERT INTO source_selector (concept_id, system_id, selector, ordinal) VALUES
  ('bokheim:business:general-reference', 'lcc', 'HF5001..HF5379.9@1', 0),
  ('bokheim:business:general-reference', 'lcc', 'HF5389.4..HF5409.9@1', 1),
  ('bokheim:business:general-reference', 'lcc', 'HF5417.6..HF5418.9@1', 2),
  ('bokheim:business:general-reference', 'lcc', 'HF5422.1..HF5427.9@1', 3),
  ('bokheim:business:general-reference', 'lcc', 'HF5430.7..HF5436.9@1', 4),
  ('bokheim:business:general-reference', 'lcc', 'HF5495.1..HF5500.9@1', 5),
  ('bokheim:business:general-reference', 'lcc', 'HF5511.24..HF5519.9@1', 6),
  ('bokheim:business:general-reference', 'lcc', 'HF5541.1..HF5545.9@1', 7),
  ('bokheim:business:general-reference', 'lcc', 'HF5548.86..HF5548.99@1', 8),
  ('bokheim:business:general-reference', 'lcc', 'HF5549.6..HF5550.9@1', 9),
  ('bokheim:business:general-reference', 'lcc', 'HF5585.1..HF5600.9@1', 10),
  ('bokheim:business:general-reference', 'lcc', 'HF5689.1..HF5690.9@1', 11),
  ('bokheim:business:general-reference', 'lcc', 'HF5734.8..HF5734.9@1', 12),
  ('bokheim:business:general-reference', 'lcc', 'HF5746.1..HF5760.9@1', 13),
  ('bokheim:business:general-reference', 'lcc', 'HF5780.1..HF5800.9@1', 14),
  ('bokheim:business:careers', 'lcc', 'HF5381..HF5386.9@1', 0),
  ('unified:c16fe56b-50ca-5c65-b34e-577d9805d17d', 'lcc', 'HF5387..HF5388.9@1', 0),
  ('bokheim:business:etiquette-broad', 'lcc', 'HF5389..HF5389.3@1', 0),
  ('unified:54299b57-991f-5cd6-9dbd-192916fefc07', 'lcc', 'HF5410..HF5417.5@1', 0),
  ('bokheim:business:wholesale', 'lcc', 'HF5419..HF5422@1', 0),
  ('bokheim:business:retail', 'lcc', 'HF5428..HF5430.6@1', 0),
  ('bokheim:business:retail', 'lcc', 'HF5459.5..HF5469.699@1', 1),
  ('unified:86a67223-84fa-5f02-b8bd-ec6404fde740', 'lcc', 'HF5437.A2..HF5437.Z@1', 0),
  ('unified:bf6d9d7e-b58f-5a84-97de-919f2ea27c43', 'lcc', 'HF5437', 0),
  ('unified:bf6d9d7e-b58f-5a84-97de-919f2ea27c43', 'lcc', 'HF5438..HF5459@1', 1),
  ('bokheim:business:markets', 'lcc', 'HF5469.7..HF5481@1', 0),
  ('bokheim:business:retail', 'lcc', 'HF5482..HF5483.9@1', 2),
  ('bokheim:business:warehousing', 'lcc', 'HF5484..HF5495@1', 0),
  ('bokheim:business:organization', 'lcc', 'HF5501..HF5511.23@1', 0),
  ('unified:38d91f55-9a6a-5498-ade0-174fa32f3958', 'lcc', 'HF5520..HF5541@1', 0),
  ('unified:1f945408-5a18-50fb-959d-3848b4b61c60', 'lcc', 'HF5546..HF5548.6@1', 0),
  ('bokheim:business:industrial-psychology', 'lcc', 'HF5548.7..HF5548.85@1', 0),
  ('bokheim:business:personnel', 'lcc', 'HF5549..HF5549.5@1', 0),
  ('unified:66cf212d-88c2-5e3a-9cd3-7b4d76217132', 'lcc', 'HF5551..HF5585@1', 0),
  ('unified:1055eec5-b7c3-519d-85d3-420d9ec68986', 'lcc', 'HF5601..HF5689@1', 0),
  ('unified:b68e0249-6e60-59ca-bcfd-7caa71559ad2', 'lcc', 'HF5691..HF5716@1', 0),
  ('unified:a7dc9eb0-abbb-5fdf-b366-c9d7c12580df', 'lcc', 'HF5717..HF5734.7@1', 0),
  ('bokheim:business:records', 'lcc', 'HF5735..HF5746@1', 0),
  ('bokheim:business:shipping', 'lcc', 'HF5761..HF5780@1', 0),
  ('unified:2adce3e6-bc0f-5d1d-b75f-bc0215566401', 'lcc', 'HF5801..HF6182@1', 0),
  ('bokheim:business:other-activities-broad', 'lcc', 'HF5599', 0),
  ('bokheim:business:special-lines-broad', 'lcc', 'HF6201', 0);

COMMIT;

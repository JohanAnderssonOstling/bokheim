BEGIN IMMEDIATE;

DELETE FROM source_selector
WHERE concept_id = 'unified:ff3e273a-ef37-56e3-b3e9-bae12b75ebab';

-- Remove the duplicate BISAC catch-all after redistributing its three codes.
DELETE FROM source_selector
WHERE concept_id = 'unified:3552bc97-3d3a-5258-946f-9f7caa4c7982';
DELETE FROM concept_parent
WHERE concept_id = 'unified:3552bc97-3d3a-5258-946f-9f7caa4c7982';
DELETE FROM source_label
WHERE concept_id = 'unified:3552bc97-3d3a-5258-946f-9f7caa4c7982';
DELETE FROM concept
WHERE concept_id = 'unified:3552bc97-3d3a-5258-946f-9f7caa4c7982';

UPDATE concept SET preferred_label = 'Industrial Relations'
WHERE concept_id = 'unified:9fc3f663-b30e-5bb3-bb87-d752fe6facd1';

INSERT OR IGNORE INTO concept (concept_id, preferred_label) VALUES
  ('bokheim:labor:general', 'Labor — General'),
  ('bokheim:labor:systems', 'Labor Systems'),
  ('bokheim:labor:wages', 'Wages & Compensation'),
  ('bokheim:labor:hours', 'Working Hours'),
  ('bokheim:labor:disputes', 'Labor Disputes & Strikes'),
  ('bokheim:labor:arbitration', 'Labor Arbitration & Mediation'),
  ('bokheim:labor:participation', 'Employee Participation'),
  ('bokheim:labor:unions', 'Trade Unions'),
  ('bokheim:labor:employers-associations', 'Employers'' Associations'),
  ('bokheim:labor:industrial-sociology', 'Industrial Sociology'),
  ('bokheim:labor:cost-of-living', 'Cost & Standard of Living'),
  ('bokheim:labor:vocational-rehabilitation', 'Vocational Rehabilitation'),
  ('bokheim:labor:health-welfare', 'Industrial Health & Welfare'),
  ('bokheim:labor:policy', 'Labor Policy'),
  ('bokheim:labor:politics', 'Labor in Politics'),
  ('bokheim:labor:professions', 'Professions & Professional Employees'),
  ('bokheim:labor:by-industry', 'Labor by Industry or Trade'),
  ('bokheim:labor:region:other', 'Labor in Other Regions');

INSERT OR IGNORE INTO concept_parent
  (concept_id, parent_concept_id, ordinal, display_order) VALUES
  ('bokheim:labor:general', 'unified:ff3e273a-ef37-56e3-b3e9-bae12b75ebab', 0, 10),
  ('bokheim:labor:systems', 'unified:ff3e273a-ef37-56e3-b3e9-bae12b75ebab', 0, 20),
  ('bokheim:labor:wages', 'unified:ff3e273a-ef37-56e3-b3e9-bae12b75ebab', 0, 30),
  ('bokheim:labor:hours', 'unified:ff3e273a-ef37-56e3-b3e9-bae12b75ebab', 0, 40),
  ('bokheim:labor:disputes', 'unified:ff3e273a-ef37-56e3-b3e9-bae12b75ebab', 0, 50),
  ('bokheim:labor:arbitration', 'unified:ff3e273a-ef37-56e3-b3e9-bae12b75ebab', 0, 60),
  ('bokheim:labor:participation', 'unified:ff3e273a-ef37-56e3-b3e9-bae12b75ebab', 0, 70),
  ('bokheim:labor:unions', 'unified:ff3e273a-ef37-56e3-b3e9-bae12b75ebab', 0, 110),
  ('bokheim:labor:employers-associations', 'unified:ff3e273a-ef37-56e3-b3e9-bae12b75ebab', 0, 120),
  ('bokheim:labor:industrial-sociology', 'unified:ff3e273a-ef37-56e3-b3e9-bae12b75ebab', 0, 130),
  ('unified:9fc3f663-b30e-5bb3-bb87-d752fe6facd1', 'unified:ff3e273a-ef37-56e3-b3e9-bae12b75ebab', 1, 140),
  ('bokheim:labor:cost-of-living', 'unified:ff3e273a-ef37-56e3-b3e9-bae12b75ebab', 0, 150),
  ('bokheim:labor:vocational-rehabilitation', 'unified:ff3e273a-ef37-56e3-b3e9-bae12b75ebab', 0, 170),
  ('bokheim:labor:health-welfare', 'unified:ff3e273a-ef37-56e3-b3e9-bae12b75ebab', 0, 180),
  ('bokheim:labor:policy', 'unified:ff3e273a-ef37-56e3-b3e9-bae12b75ebab', 0, 190),
  ('bokheim:labor:politics', 'unified:ff3e273a-ef37-56e3-b3e9-bae12b75ebab', 0, 200),
  ('bokheim:labor:professions', 'unified:ff3e273a-ef37-56e3-b3e9-bae12b75ebab', 0, 210),
  ('bokheim:labor:by-industry', 'unified:ff3e273a-ef37-56e3-b3e9-bae12b75ebab', 0, 220),
  ('bokheim:labor:region:other', 'unified:ff3e273a-ef37-56e3-b3e9-bae12b75ebab', 0, 300);

DELETE FROM source_selector
WHERE concept_id IN ('bokheim:labor:employment','bokheim:labor:worker-groups',
                     'bokheim:labor:social-insurance',
                     'unified:9fc3f663-b30e-5bb3-bb87-d752fe6facd1')
  AND system_id='lcc';

INSERT INTO source_selector (concept_id, system_id, selector, ordinal) VALUES
  ('bokheim:labor:general', 'bisac', 'BUS038000', 0),
  ('bokheim:labor:general', 'lcc', 'HD4801..HD4860.9@1', 0),
  ('bokheim:labor:general', 'lcc', 'HD4895.1..HD4908.9@1', 1),
  ('bokheim:labor:general', 'lcc', 'HD5100.91..HD5105.9@1', 2),
  ('bokheim:labor:general', 'lcc', 'HD5267.1..HD5305.9@1', 3),
  ('bokheim:labor:general', 'lcc', 'HD5474.1..HD5480.9@1', 4),
  ('bokheim:labor:general', 'lcc', 'HD5630.71..HD5649.9@1', 5),
  ('bokheim:labor:general', 'lcc', 'HD5660.1..HD5700.9@1', 6),
  ('bokheim:labor:general', 'lcc', 'HD6001..HD6049.9@1', 7),
  ('bokheim:labor:general', 'lcc', 'HD6305.1..HD6349.9@1', 8),
  ('bokheim:labor:general', 'lcc', 'HD6948.1..HD6950.9@1', 9),
  ('bokheim:labor:general', 'lcc', 'HD6957.1..HD6958.49@1', 10),
  ('bokheim:labor:general', 'lcc', 'HD6976.1..HD6976.9@1', 11),
  ('bokheim:labor:general', 'lcc', 'HD7080.1..HD7087.9@1', 12),
  ('bokheim:labor:general', 'lcc', 'HD7252.1..HD7254.9@1', 13),
  ('bokheim:labor:general', 'lcc', 'HD7256.1..HD7259.9@1', 14),
  ('bokheim:labor:general', 'lcc', 'HD7780.81..HD7794.9@1', 15),
  ('bokheim:labor:general', 'lcc', 'HD8027.1..HD8030.9@1', 16),
  ('bokheim:labor:general', 'lcc', 'HD8031.1..HD8037.9@1', 17),
  ('bokheim:labor:systems', 'lcc', 'HD4861..HD4895@1', 0),
  ('bokheim:labor:wages', 'bisac', 'BUS038020', 0),
  ('bokheim:labor:wages', 'lcc', 'HD4909..HD5100.9@1', 0),
  ('bokheim:labor:hours', 'lcc', 'HD5106..HD5267@1', 0),
  ('bokheim:labor:disputes', 'lcc', 'HD5306..HD5474@1', 0),
  ('bokheim:labor:arbitration', 'lcc', 'HD5481..HD5630.7@1', 0),
  ('bokheim:labor:participation', 'lcc', 'HD5650..HD5660@1', 0),
  ('bokheim:labor:employment', 'lcc', 'HD5701..HD6000.9@1', 0),
  ('bokheim:labor:worker-groups', 'lcc', 'HD6050..HD6305@1', 0),
  ('bokheim:labor:unions', 'bisac', 'BUS038010', 0),
  ('bokheim:labor:unions', 'lcc', 'HD6350..HD6940.7@1', 0),
  ('bokheim:labor:employers-associations', 'lcc', 'HD6941..HD6948@1', 0),
  ('bokheim:labor:industrial-sociology', 'lcc', 'HD6951..HD6957@1', 0),
  ('unified:9fc3f663-b30e-5bb3-bb87-d752fe6facd1', 'lcc', 'HD6958.5..HD6976@1', 0),
  ('bokheim:labor:cost-of-living', 'lcc', 'HD6977..HD7080@1', 0),
  ('bokheim:labor:social-insurance', 'lcc', 'HD7088..HD7252@1', 0),
  ('bokheim:labor:vocational-rehabilitation', 'lcc', 'HD7255..HD7256@1', 0),
  ('bokheim:labor:health-welfare', 'lcc', 'HD7260..HD7780.8@1', 0),
  ('bokheim:labor:policy', 'lcc', 'HD7795..HD8027@1', 0),
  ('bokheim:labor:politics', 'lcc', 'HD8031', 0),
  ('bokheim:labor:politics', 'lcc', 'HD8031.A1..HD8031.Z@1', 1),
  ('bokheim:labor:professions', 'lcc', 'HD8038', 0),
  ('bokheim:labor:professions', 'lcc', 'HD8038.A1..HD8038.Z@1', 1),
  ('bokheim:labor:by-industry', 'lcc', 'HD8039', 0),
  ('bokheim:labor:by-industry', 'lcc', 'HD8039.A1..HD8039.Z@1', 1),
  ('bokheim:labor:by-industry', 'lcc', 'HD8039.0001..HD8039.9999@1', 2),
  ('bokheim:labor:region:other', 'lcc', 'HD8045..HD8050.9@1', 0),
  ('bokheim:labor:region:other', 'lcc', 'HD8085.1..HD8100.9@1', 1),
  ('bokheim:labor:region:other', 'lcc', 'HD8110.1..HD8110.49@1', 2),
  ('bokheim:labor:region:other', 'lcc', 'HD8650.7..HD8650.9@1', 3),
  ('bokheim:labor:region:other', 'lcc', 'HD8755.1..HD8770.9@1', 4),
  ('bokheim:labor:region:other', 'lcc', 'HD8837.1..HD8840.9@1', 5),
  ('bokheim:labor:region:other', 'lcc', 'HD8850.1..HD8930.49@1', 6),
  ('bokheim:labor:region:other', 'lcc', 'HD8939.6..HD8942.49@1', 7),
  ('bokheim:labor:region:other', 'lcc', 'HD8942.51..HD8943@1', 8);

COMMIT;

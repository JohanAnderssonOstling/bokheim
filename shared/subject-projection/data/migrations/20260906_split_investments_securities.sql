BEGIN IMMEDIATE;

DELETE FROM source_selector
WHERE concept_id IN (
  'unified:8fd0add6-3e26-5d58-9e43-eec09368decc',
  'bokheim:investments:general',
  'bokheim:investments:analysis-portfolio',
  'bokheim:investments:funds',
  'bokheim:investments:foreign',
  'bokheim:investments:exchanges',
  'bokheim:investments:brokers-markets',
  'bokheim:investments:fixed-income',
  'bokheim:investments:stocks',
  'bokheim:investments:government-securities',
  'bokheim:investments:venture-other-securities',
  'bokheim:investments:by-region',
  'bokheim:investments:speculation-derivatives',
  'bokheim:investments:commodities',
  'bokheim:investments:real-estate'
);

-- This BISAC code denotes the combined parent exactly.
INSERT INTO source_selector (concept_id, system_id, selector, ordinal) VALUES
  ('unified:8fd0add6-3e26-5d58-9e43-eec09368decc', 'bisac', 'BUS036000', 0);

INSERT OR IGNORE INTO concept (concept_id, preferred_label) VALUES
  ('bokheim:investments:general', 'General Investing'),
  ('bokheim:investments:analysis-portfolio', 'Investment Analysis & Portfolio Management'),
  ('bokheim:investments:funds', 'Funds & Investment Companies'),
  ('bokheim:investments:foreign', 'Foreign Investment'),
  ('bokheim:investments:exchanges', 'Stock Exchanges'),
  ('bokheim:investments:brokers-markets', 'Brokers & Securities Markets'),
  ('bokheim:investments:fixed-income', 'Bonds & Fixed-Income Securities'),
  ('bokheim:investments:stocks', 'Stocks'),
  ('bokheim:investments:government-securities', 'Government Securities'),
  ('bokheim:investments:venture-other-securities', 'Venture Capital & Other Securities'),
  ('bokheim:investments:by-region', 'Investment by Region'),
  ('bokheim:investments:speculation-derivatives', 'Speculation, Futures & Derivatives'),
  ('bokheim:investments:commodities', 'Commodities'),
  ('bokheim:investments:real-estate', 'Real-Estate Investing');

INSERT OR IGNORE INTO concept_parent
  (concept_id, parent_concept_id, ordinal, display_order) VALUES
  ('bokheim:investments:general', 'unified:8fd0add6-3e26-5d58-9e43-eec09368decc', 0, 10),
  ('bokheim:investments:analysis-portfolio', 'unified:8fd0add6-3e26-5d58-9e43-eec09368decc', 0, 20),
  ('bokheim:investments:funds', 'unified:8fd0add6-3e26-5d58-9e43-eec09368decc', 0, 30),
  ('bokheim:investments:foreign', 'unified:8fd0add6-3e26-5d58-9e43-eec09368decc', 0, 40),
  ('bokheim:investments:exchanges', 'unified:8fd0add6-3e26-5d58-9e43-eec09368decc', 0, 50),
  ('bokheim:investments:brokers-markets', 'unified:8fd0add6-3e26-5d58-9e43-eec09368decc', 0, 60),
  ('bokheim:investments:fixed-income', 'unified:8fd0add6-3e26-5d58-9e43-eec09368decc', 0, 70),
  ('bokheim:investments:stocks', 'unified:8fd0add6-3e26-5d58-9e43-eec09368decc', 0, 80),
  ('bokheim:investments:government-securities', 'unified:8fd0add6-3e26-5d58-9e43-eec09368decc', 0, 90),
  ('bokheim:investments:venture-other-securities', 'unified:8fd0add6-3e26-5d58-9e43-eec09368decc', 0, 100),
  ('bokheim:investments:by-region', 'unified:8fd0add6-3e26-5d58-9e43-eec09368decc', 0, 110),
  ('bokheim:investments:speculation-derivatives', 'unified:8fd0add6-3e26-5d58-9e43-eec09368decc', 0, 120),
  ('bokheim:investments:commodities', 'unified:8fd0add6-3e26-5d58-9e43-eec09368decc', 0, 130),
  ('bokheim:investments:real-estate', 'unified:8fd0add6-3e26-5d58-9e43-eec09368decc', 0, 140);

INSERT INTO source_selector (concept_id, system_id, selector, ordinal) VALUES
  ('bokheim:investments:general', 'lcc', 'HG4501..HG4528.9@1', 0),
  ('bokheim:investments:analysis-portfolio', 'bisac', 'BUS036070', 0),
  ('bokheim:investments:analysis-portfolio', 'bisac', 'BUS036090', 1),
  ('bokheim:investments:analysis-portfolio', 'lcc', 'HG4529..HG4529.9@1', 0),
  ('bokheim:investments:funds', 'bisac', 'BUS036030', 0),
  ('bokheim:investments:funds', 'lcc', 'HG4530..HG4537.9@1', 0),
  ('bokheim:investments:foreign', 'lcc', 'HG4538..HG4550.9@1', 0),
  ('bokheim:investments:exchanges', 'lcc', 'HG4551..HG4598.9@1', 0),
  ('bokheim:investments:brokers-markets', 'lcc', 'HG4599..HG4649.9@1', 0),
  ('bokheim:investments:fixed-income', 'bisac', 'BUS036010', 0),
  ('bokheim:investments:fixed-income', 'lcc', 'HG4650..HG4660.9@1', 0),
  ('bokheim:investments:stocks', 'bisac', 'BUS036060', 0),
  ('bokheim:investments:stocks', 'lcc', 'HG4661..HG4700.9@1', 0),
  ('bokheim:investments:government-securities', 'lcc', 'HG4701..HG4750.9@1', 0),
  ('bokheim:investments:venture-other-securities', 'lcc', 'HG4751..HG4899.9@1', 0),
  ('bokheim:investments:by-region', 'lcc', 'HG4900..HG5999.9@1', 0),
  ('bokheim:investments:speculation-derivatives', 'bisac', 'BUS036020', 0),
  ('bokheim:investments:speculation-derivatives', 'bisac', 'BUS036040', 1),
  ('bokheim:investments:speculation-derivatives', 'bisac', 'BUS036080', 2),
  ('bokheim:investments:speculation-derivatives', 'lcc', 'HG6001..HG6052.Z@1', 0),
  ('bokheim:investments:commodities', 'bisac', 'BUS014000', 0),
  ('bokheim:investments:commodities', 'bisac', 'BUS014010', 1),
  ('bokheim:investments:commodities', 'bisac', 'BUS014020', 2),
  ('bokheim:investments:real-estate', 'bisac', 'BUS036050', 0);

COMMIT;

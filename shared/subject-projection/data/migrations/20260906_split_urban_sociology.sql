BEGIN IMMEDIATE;

UPDATE concept
SET preferred_label = 'Cities & Urban Studies'
WHERE concept_id = 'unified:c217d07b-6ce3-5745-8b7a-8aa0a6f9dd99';

DELETE FROM source_selector
WHERE concept_id = 'unified:c217d07b-6ce3-5745-8b7a-8aa0a6f9dd99';

-- SOC026030 is specifically urban sociology. Extract it from the general
-- Sociology concept and combine it with the relevant HT classifications.
DELETE FROM source_selector
WHERE concept_id = 'unified:1dc4999e-4875-54c4-be21-e8b23a259be2'
  AND system_id = 'bisac'
  AND selector = 'SOC026030';

INSERT OR IGNORE INTO concept (concept_id, preferred_label) VALUES
  ('bokheim:urban:sociology', 'Urban Sociology'),
  ('bokheim:urban:garden-cities', 'Garden Cities'),
  ('bokheim:urban:renewal', 'Urban Renewal & Redevelopment'),
  ('bokheim:urban:population', 'Urban Population'),
  ('bokheim:urban:life', 'Urban Life'),
  ('bokheim:urban:culture-recreation', 'Urban Culture & Recreation'),
  ('bokheim:urban:economics', 'Urban Economics'),
  ('bokheim:urban:metropolitan-areas', 'Metropolitan Areas'),
  ('bokheim:urban:suburbs', 'Suburbs & Suburban Communities'),
  ('bokheim:urban:urbanization', 'Urbanization'),
  ('bokheim:urban:regional-economics', 'Regional Economics');

INSERT OR IGNORE INTO concept_parent
  (concept_id, parent_concept_id, ordinal, display_order) VALUES
  ('bokheim:urban:sociology', 'unified:c217d07b-6ce3-5745-8b7a-8aa0a6f9dd99', 0, 10),
  ('bokheim:urban:garden-cities', 'unified:c217d07b-6ce3-5745-8b7a-8aa0a6f9dd99', 0, 20),
  ('unified:29e51cdb-476a-56ad-a8d9-92aec90bfcf7', 'unified:c217d07b-6ce3-5745-8b7a-8aa0a6f9dd99', 1, 30),
  ('bokheim:urban:renewal', 'unified:c217d07b-6ce3-5745-8b7a-8aa0a6f9dd99', 0, 40),
  ('bokheim:urban:population', 'unified:c217d07b-6ce3-5745-8b7a-8aa0a6f9dd99', 0, 50),
  ('bokheim:urban:life', 'unified:c217d07b-6ce3-5745-8b7a-8aa0a6f9dd99', 0, 60),
  ('bokheim:urban:culture-recreation', 'unified:c217d07b-6ce3-5745-8b7a-8aa0a6f9dd99', 0, 70),
  ('bokheim:urban:economics', 'unified:c217d07b-6ce3-5745-8b7a-8aa0a6f9dd99', 0, 80),
  ('bokheim:urban:metropolitan-areas', 'unified:c217d07b-6ce3-5745-8b7a-8aa0a6f9dd99', 0, 90),
  ('bokheim:urban:suburbs', 'unified:c217d07b-6ce3-5745-8b7a-8aa0a6f9dd99', 0, 100),
  ('bokheim:urban:urbanization', 'unified:c217d07b-6ce3-5745-8b7a-8aa0a6f9dd99', 0, 110),
  ('bokheim:urban:regional-economics', 'unified:c217d07b-6ce3-5745-8b7a-8aa0a6f9dd99', 0, 120),
  ('bokheim:politics:policy:regional-planning', 'unified:c217d07b-6ce3-5745-8b7a-8aa0a6f9dd99', 1, 130);

INSERT INTO source_selector (concept_id, system_id, selector, ordinal) VALUES
  ('bokheim:urban:sociology', 'bisac', 'SOC026030', 0),
  ('bokheim:urban:sociology', 'lcc', 'HT101..HT160.9@1', 0),
  ('bokheim:urban:garden-cities', 'lcc', 'HT161..HT165.49@1', 0),
  ('unified:29e51cdb-476a-56ad-a8d9-92aec90bfcf7', 'lcc', 'HT165.5..HT169.9@1', 0),
  ('bokheim:urban:renewal', 'lcc', 'HT170..HT200.9@1', 0),
  ('bokheim:urban:population', 'lcc', 'HT201..HT230.9@1', 0),
  ('bokheim:urban:life', 'lcc', 'HT231..HT250.9@1', 0),
  ('bokheim:urban:culture-recreation', 'lcc', 'HT251..HT320.9@1', 0),
  ('bokheim:urban:economics', 'lcc', 'HT321..HT329.9@1', 0),
  ('bokheim:urban:metropolitan-areas', 'lcc', 'HT330..HT350.9@1', 0),
  ('bokheim:urban:suburbs', 'lcc', 'HT351..HT360.9@1', 0),
  ('bokheim:urban:urbanization', 'lcc', 'HT361..HT387.9@1', 0),
  ('bokheim:urban:regional-economics', 'lcc', 'HT388..HT389.9@1', 0),
  ('bokheim:politics:policy:regional-planning', 'lcc', 'HT390..HT395@1', 0);

COMMIT;

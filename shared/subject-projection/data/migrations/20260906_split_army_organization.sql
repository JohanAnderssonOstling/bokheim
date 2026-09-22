BEGIN IMMEDIATE;

DELETE FROM source_selector
WHERE concept_id = 'unified:8235dfd5-1997-5968-b4a0-8d3d6964c615';

-- Keep the class-level notation exact on the meaningful non-leaf.  All
-- permissive coverage belongs to the leaves below.
INSERT INTO source_selector (concept_id, system_id, selector, ordinal) VALUES
  ('unified:8235dfd5-1997-5968-b4a0-8d3d6964c615', 'lcc', 'UA', 0);

INSERT OR IGNORE INTO concept (concept_id, preferred_label) VALUES
  ('bokheim:army-organization:general', 'Army Organization'),
  ('bokheim:army-organization:by-country', 'Armies by Country'),
  ('bokheim:army-organization:mobilization', 'Military Mobilization'),
  ('bokheim:army-organization:defense-planning', 'Attack & Defense Planning'),
  ('bokheim:army-organization:civil-defense', 'Civil & Industrial Defense'),
  ('bokheim:army-organization:communications', 'Military Communications'),
  ('bokheim:army-organization:routes', 'Military Routes & Transportation'),
  ('bokheim:army-organization:geography', 'Military Geography');

INSERT OR IGNORE INTO concept_parent
  (concept_id, parent_concept_id, ordinal, display_order) VALUES
  ('bokheim:army-organization:general', 'unified:8235dfd5-1997-5968-b4a0-8d3d6964c615', 0, 10),
  ('bokheim:army-organization:by-country', 'unified:8235dfd5-1997-5968-b4a0-8d3d6964c615', 0, 20),
  ('bokheim:army-organization:mobilization', 'unified:8235dfd5-1997-5968-b4a0-8d3d6964c615', 0, 30),
  ('bokheim:army-organization:defense-planning', 'unified:8235dfd5-1997-5968-b4a0-8d3d6964c615', 0, 40),
  ('bokheim:army-organization:civil-defense', 'unified:8235dfd5-1997-5968-b4a0-8d3d6964c615', 0, 50),
  ('bokheim:army-organization:communications', 'unified:8235dfd5-1997-5968-b4a0-8d3d6964c615', 0, 60),
  ('bokheim:army-organization:routes', 'unified:8235dfd5-1997-5968-b4a0-8d3d6964c615', 0, 70),
  ('bokheim:army-organization:geography', 'unified:8235dfd5-1997-5968-b4a0-8d3d6964c615', 0, 80);

INSERT INTO source_selector (concept_id, system_id, selector, ordinal) VALUES
  ('bokheim:army-organization:general', 'lcc', 'UA10..UA20.9@1', 0),
  ('bokheim:army-organization:by-country', 'lcc', 'UA21..UA909.9@1', 0),
  ('bokheim:army-organization:mobilization', 'lcc', 'UA910..UA919.9@1', 0),
  ('bokheim:army-organization:defense-planning', 'lcc', 'UA920..UA925.9@1', 0),
  ('bokheim:army-organization:civil-defense', 'lcc', 'UA926..UA939.9@1', 0),
  ('bokheim:army-organization:communications', 'lcc', 'UA940..UA949.9@1', 0),
  ('bokheim:army-organization:routes', 'lcc', 'UA950..UA984.9@1', 0),
  ('bokheim:army-organization:geography', 'lcc', 'UA985..UA997.Z@1', 0);

COMMIT;

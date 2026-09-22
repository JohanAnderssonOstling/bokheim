BEGIN IMMEDIATE;

DELETE FROM source_selector
WHERE concept_id = 'unified:fc1f8eb8-d687-5ea4-969b-2efb0c169828';

-- These are genuinely general source classifications and therefore remain
-- exact selectors on the meaningful Computer Science non-leaf.
INSERT INTO source_selector (concept_id, system_id, selector, ordinal) VALUES
  ('unified:fc1f8eb8-d687-5ea4-969b-2efb0c169828', 'bisac', 'COM014000', 0),
  ('unified:fc1f8eb8-d687-5ea4-969b-2efb0c169828', 'lcc', 'QA75.5-76.95', 0),
  ('unified:fc1f8eb8-d687-5ea4-969b-2efb0c169828', 'lcc', 'QA76', 1);

INSERT OR IGNORE INTO concept (concept_id, preferred_label) VALUES
  ('bokheim:computer-science:reference-education', 'Reference, Communication & Education'),
  ('bokheim:computer-science:computer-systems', 'Analog & Digital Computer Systems'),
  ('bokheim:computer-science:advanced-architectures', 'Advanced Computer Architectures');

INSERT OR IGNORE INTO concept_parent
  (concept_id, parent_concept_id, ordinal, display_order) VALUES
  ('bokheim:computer-science:reference-education', 'unified:fc1f8eb8-d687-5ea4-969b-2efb0c169828', 0, 20),
  ('bokheim:computer-science:computer-systems', 'unified:fc1f8eb8-d687-5ea4-969b-2efb0c169828', 0, 30),
  ('bokheim:computer-science:advanced-architectures', 'unified:fc1f8eb8-d687-5ea4-969b-2efb0c169828', 0, 40);

INSERT INTO source_selector (concept_id, system_id, selector, ordinal) VALUES
  ('bokheim:computer-science:reference-education', 'lcc', 'QA75.5', 0),
  ('bokheim:computer-science:reference-education', 'lcc', 'QA76.15', 1),
  ('bokheim:computer-science:reference-education', 'lcc', 'QA76.16', 2),
  ('bokheim:computer-science:reference-education', 'lcc', 'QA76.162', 3),
  ('bokheim:computer-science:reference-education', 'lcc', 'QA76.165', 4),
  ('bokheim:computer-science:reference-education', 'lcc', 'QA76.215', 5),
  ('bokheim:computer-science:reference-education', 'lcc', 'QA76.23', 6),
  ('bokheim:computer-science:reference-education', 'lcc', 'QA76.24', 7),
  ('bokheim:computer-science:reference-education', 'lcc', 'QA76.33', 8),
  ('bokheim:computer-science:reference-education', 'lcc', 'QA76.35', 9),

  ('bokheim:computer-science:computer-systems', 'lcc', 'QA76.4', 0),
  ('bokheim:computer-science:computer-systems', 'lcc', 'QA76.525', 1),
  ('bokheim:computer-science:computer-systems', 'lcc', 'QA76.527', 2),
  ('bokheim:computer-science:computer-systems', 'lcc', 'QA76.529', 3),
  ('bokheim:computer-science:computer-systems', 'lcc', 'QA76.5295.A34', 4),
  ('bokheim:computer-science:computer-systems', 'lcc', 'QA76.5295.P37', 5),
  ('bokheim:computer-science:computer-systems', 'lcc', 'QA76.53', 6),

  ('bokheim:computer-science:advanced-architectures', 'lcc', 'QA76.84', 0),
  ('bokheim:computer-science:advanced-architectures', 'lcc', 'QA76.85', 1),
  ('bokheim:computer-science:advanced-architectures', 'lcc', 'QA76.875', 2),
  ('bokheim:computer-science:advanced-architectures', 'lcc', 'QA76.884', 3),
  ('bokheim:computer-science:advanced-architectures', 'lcc', 'QA76.885', 4),
  ('bokheim:computer-science:advanced-architectures', 'lcc', 'QA76.89', 5);

-- QA76.A3eb was deliberately not migrated: it is absent from the official
-- LCC definitions and contains a malformed lowercase ebook suffix.

COMMIT;

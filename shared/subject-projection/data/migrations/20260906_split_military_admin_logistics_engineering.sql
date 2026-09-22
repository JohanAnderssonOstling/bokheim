BEGIN IMMEDIATE;

DELETE FROM source_selector
WHERE concept_id = 'bokheim:military-studies:administration-logistics';

INSERT OR IGNORE INTO concept (concept_id, preferred_label) VALUES
  ('bokheim:military-studies:military-administration', 'Military Administration'),
  ('bokheim:military-studies:military-logistics', 'Military Logistics & Supply'),
  ('bokheim:military-studies:military-engineering', 'Military Engineering');

INSERT OR IGNORE INTO concept_parent
  (concept_id, parent_concept_id, ordinal, display_order) VALUES
  ('bokheim:military-studies:military-administration', 'bokheim:military-studies:administration-logistics', 0, 10),
  ('bokheim:military-studies:military-logistics', 'bokheim:military-studies:administration-logistics', 0, 20),
  ('bokheim:military-studies:military-engineering', 'bokheim:military-studies:administration-logistics', 0, 30);

-- UB, UC, and UG are separate LCC classes. Each is intentionally represented
-- by one curated leaf rather than hundreds of generated subdivisions.
INSERT INTO source_selector (concept_id, system_id, selector, ordinal) VALUES
  ('bokheim:military-studies:military-administration', 'lcc', 'UB', 0),
  ('bokheim:military-studies:military-administration', 'lcc', 'UB1..UB900@1', 1),
  ('bokheim:military-studies:military-logistics', 'lcc', 'UC', 0),
  ('bokheim:military-studies:military-logistics', 'lcc', 'UC10..UC780@1', 1),
  ('bokheim:military-studies:military-engineering', 'lcc', 'UG1..UG620@1', 0);

COMMIT;

BEGIN IMMEDIATE;

UPDATE concept
SET preferred_label = 'Agricultural Economics'
WHERE concept_id = 'unified:c92f48be-7525-56da-9f95-9c4b98aedce1';

DELETE FROM source_selector
WHERE concept_id = 'unified:c92f48be-7525-56da-9f95-9c4b98aedce1';

INSERT OR IGNORE INTO concept (concept_id, preferred_label) VALUES
  ('bokheim:agricultural-economics:general', 'General Agricultural Economics'),
  ('bokheim:agricultural-economics:farm-structure', 'Farm Structure & Cooperation'),
  ('bokheim:agricultural-economics:labor-tenancy', 'Farm Tenancy & Agricultural Labor'),
  ('bokheim:agricultural-economics:land-water', 'Agricultural Land & Water Development'),
  ('bokheim:agricultural-economics:irrigation', 'Irrigation'),
  ('bokheim:agricultural-economics:by-region', 'Agricultural Economics by Region');

INSERT OR IGNORE INTO concept_parent
  (concept_id, parent_concept_id, ordinal, display_order) VALUES
  ('bokheim:agricultural-economics:general', 'unified:c92f48be-7525-56da-9f95-9c4b98aedce1', 0, 10),
  ('bokheim:agricultural-economics:farm-structure', 'unified:c92f48be-7525-56da-9f95-9c4b98aedce1', 0, 20),
  ('bokheim:agricultural-economics:labor-tenancy', 'unified:c92f48be-7525-56da-9f95-9c4b98aedce1', 0, 30),
  ('bokheim:agricultural-economics:land-water', 'unified:c92f48be-7525-56da-9f95-9c4b98aedce1', 0, 40),
  ('bokheim:agricultural-economics:irrigation', 'unified:c92f48be-7525-56da-9f95-9c4b98aedce1', 0, 50),
  ('bokheim:agricultural-economics:by-region', 'unified:c92f48be-7525-56da-9f95-9c4b98aedce1', 0, 60);

-- Broad ranges are intentional here: each target is a curated leaf replacing
-- a large official detail tree, and the ranges form one non-overlapping
-- partition of the former HD1401..HD2210 coverage.
INSERT INTO source_selector (concept_id, system_id, selector, ordinal) VALUES
  ('bokheim:agricultural-economics:general', 'lcc', 'HD1401..HD1469.9@1', 0),
  ('bokheim:agricultural-economics:farm-structure', 'lcc', 'HD1470..HD1509.9@1', 0),
  ('bokheim:agricultural-economics:labor-tenancy', 'lcc', 'HD1510..HD1579.9@1', 0),
  ('bokheim:agricultural-economics:land-water', 'lcc', 'HD1580..HD1709.9@1', 0),
  ('bokheim:agricultural-economics:irrigation', 'lcc', 'HD1710..HD1747.9@1', 0),
  ('bokheim:agricultural-economics:by-region', 'lcc', 'HD1748..HD2210.Z@1', 0);

COMMIT;

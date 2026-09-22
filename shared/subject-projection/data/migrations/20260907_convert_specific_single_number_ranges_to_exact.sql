-- Specific single-number topics do not need permissive ranges: numbered exact
-- selectors also match their Cutter descendants. Broad residual ranges remain
-- on the meaningful parent subjects.

DELETE FROM lcc_range
WHERE (concept_id = 53354 AND start_code = 'QA273.A1' AND end_code = 'QA273.Z')
   OR (concept_id = 53373 AND start_code = 'TA418.9.A' AND end_code = 'TA418.9.Z')
   OR (concept_id = 53375 AND start_code = 'TA660.A' AND end_code = 'TA660.Z')
   OR (concept_id = 53387 AND start_code = 'TH6124' AND end_code = 'TH6124.99')
   OR (concept_id = 53389 AND start_code = 'TS1445' AND end_code = 'TS1445.99');

INSERT OR IGNORE INTO lcc_selector(concept_id, selector) VALUES
  (53354, 'QA273'),
  (53373, 'TA418.9'),
  (53375, 'TA660'),
  (53387, 'TH6124'),
  (53389, 'TS1445');

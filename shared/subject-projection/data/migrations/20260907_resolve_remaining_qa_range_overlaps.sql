-- Split broad leaf ranges around their more specific sibling ranges. Printed
-- official divisions remain available as exact selectors.

DELETE FROM lcc_range WHERE concept_id IN (4921,13331,4947);

INSERT INTO lcc_range(start_code,end_code,concept_id)
VALUES
  ('QA184','QA187.Z',4921),
  ('QA196.8','QA205',4921),
  ('QA241','QA245.Z',13331),
  ('QA8','QA8.Z',4947),
  ('QA10.31','QA10.4',4947);

INSERT OR IGNORE INTO lcc_selector(concept_id,selector)
VALUES
  (4921,'QA184-205'),
  (13331,'QA241-247.5'),
  (4947,'QA8-10.4');

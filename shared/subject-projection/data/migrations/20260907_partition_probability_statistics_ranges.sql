-- Add the missing probability-theory division and partition mathematical
-- statistics around its existing specialist leaves.

INSERT INTO concept(concept_id,preferred_label)
VALUES
  (53354,'Probability Theory'),
  (53355,'Mathematical Statistics Reference & Methods');

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal)
VALUES
  (53354,4961,0),
  (53355,4961,0);

INSERT OR IGNORE INTO lcc_selector(concept_id,selector)
SELECT 53355,selector FROM lcc_selector WHERE concept_id=4961;
DELETE FROM lcc_selector WHERE concept_id=4961;

DELETE FROM lcc_range WHERE concept_id=4961;

INSERT INTO lcc_range(start_code,end_code,concept_id)
VALUES
  ('QA273.A1','QA273.Z',53354),
  ('QA276','QA276.155',53355),
  ('QA276.158','QA276.39',53355),
  ('QA276.46','QA276.99',53355),
  ('QA277.4','QA277.9',53355),
  ('QA278.66','QA278.99',53355);

INSERT OR IGNORE INTO lcc_selector(concept_id,selector)
VALUES
  (53354,'QA273'),
  (4961,'QA273.A1-274.9'),
  (4961,'QA276-280'),
  (4961,'QA275'),
  (4964,'QA278.2'),
  (4962,'QA279.5');

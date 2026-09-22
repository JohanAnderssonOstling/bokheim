-- Keep only the densely occupied reference/theory opening of the schedule as
-- residual range coverage. Later valid headings are already represented by
-- specialist ranges or exact selectors; the intervening numbers are unused.

INSERT INTO concept(concept_id,preferred_label)
VALUES (53340,'Physical Chemistry Reference & Theory');

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal)
VALUES (53340,4808,0);

DELETE FROM lcc_range WHERE concept_id=4808;

INSERT INTO lcc_range(start_code,end_code,concept_id)
VALUES
  ('QD450','QD451.Z',53340),
  ('QD452.6','QD461.9',53340);

-- This official two-number span has negligible use and does not merit a leaf.
INSERT OR IGNORE INTO lcc_selector(concept_id,selector)
VALUES
  (4808,'QD463'),
  (4808,'QD464');

-- The apparent gaps in QD241-QD441 are unassigned schedule space, except for
-- the isolated QD291 heading and QD441 (already an exact child). Do not cover
-- those gaps with an artificial umbrella.

INSERT INTO concept(concept_id,preferred_label)
VALUES (53339,'Organic Chemistry Reference, Study & Methods');

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal)
VALUES (53339,4807,0);

DELETE FROM lcc_range WHERE concept_id=4807;

INSERT INTO lcc_range(start_code,end_code,concept_id)
VALUES
  ('QD241','QD247.Z',53339),
  ('QD248.6','QD257.5',53339);

INSERT OR IGNORE INTO lcc_selector(concept_id,selector)
VALUES (4807,'QD291');

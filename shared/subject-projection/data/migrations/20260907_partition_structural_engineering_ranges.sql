-- Complete TA630-TA695 around the existing computational, analysis, design,
-- and materials leaves.

INSERT INTO concept(concept_id,preferred_label)
VALUES
  (53374,'Structural Engineering Reference & Early Works'),
  (53375,'Structural Forms');

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal)
VALUES
  (53374,292,0),
  (53375,292,0);

DELETE FROM lcc_range WHERE concept_id=292;

INSERT INTO lcc_range(start_code,end_code,concept_id)
VALUES
  ('TA630','TA639.9',53374),
  ('TA660.A','TA660.Z',53375);

INSERT OR IGNORE INTO lcc_selector(concept_id,selector)
VALUES (292,'TA630-695');

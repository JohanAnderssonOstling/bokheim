-- Partition the occupied residual portions of QD901-QD999 around the existing
-- history, biography, mathematical, structural, physical, and chemical leaves.

INSERT INTO concept(concept_id,preferred_label)
VALUES (53341,'Crystallography Reference, Study & Methods');

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal)
VALUES (53341,4805,0);

DELETE FROM lcc_range WHERE concept_id=4805;

INSERT INTO lcc_range(start_code,end_code,concept_id)
VALUES
  ('QD901','QD902.9',53341),
  ('QD903.7','QD910',53341);

INSERT OR IGNORE INTO lcc_selector(concept_id,selector)
VALUES (4805,'QD999');

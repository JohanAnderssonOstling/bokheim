-- Partition the broadly used but individually sparse TA1001-TA1280 division
-- around its existing specialist leaves.

INSERT INTO concept(concept_id,preferred_label)
VALUES (53381,'Transportation Engineering Reference & Regional Studies');

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal)
VALUES (53381,237,0);

DELETE FROM lcc_range WHERE concept_id=237;

INSERT INTO lcc_range(start_code,end_code,concept_id)
VALUES
  ('TA1001','TA1204.9',53381),
  ('TA1207.1','TA1209.9',53381),
  ('TA1215.1','TA1244.9',53381),
  ('TA1250.1','TA1280',53381);

INSERT OR IGNORE INTO lcc_selector(concept_id,selector)
VALUES
  (237,'TA1001-1280'),
  (237,'TA1021-1127');

-- Remove permissive ranges from Technological Change and Industrial R&D;
-- their valid headings are represented by exact children and one regional leaf.

DELETE FROM lcc_range WHERE concept_id IN (452,447,450);

INSERT OR IGNORE INTO lcc_selector(concept_id,selector)
VALUES
  (452,'T173.2-174.5'),
  (452,'T173.2'),
  (452,'T174.2'),
  (447,'T175-178'),
  (447,'T175'),
  (447,'T178');

-- Patents before the regional-history sequence are reference, agreements,
-- literature, theory, and practice. Existing exact topic children override it.
INSERT INTO concept(concept_id,preferred_label)
VALUES (53367,'Patents Reference & Practice');

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal)
VALUES (53367,450,0);

INSERT INTO lcc_range(start_code,end_code,concept_id)
VALUES ('T201','T214.9',53367);

INSERT OR IGNORE INTO lcc_selector(concept_id,selector)
VALUES (450,'T201-342');

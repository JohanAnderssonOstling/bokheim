-- Move the two reference/study spans off the detailed nonleaf. Its exact
-- selectors and specialist children continue to cover the rest of QC120-168.86.

INSERT INTO concept(concept_id,preferred_label)
VALUES (53342,'Experimental Mechanics Reference & Study');

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal)
VALUES (53342,5033,0);

DELETE FROM lcc_range WHERE concept_id=5033;

INSERT INTO lcc_range(start_code,end_code,concept_id)
VALUES
  ('QC125','QC125.2',53342),
  ('QC128','QC129.5',53342);

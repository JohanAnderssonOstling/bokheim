-- Existing specialist leaves cover all occupied atomic-physics divisions after
-- the opening reference/textbook span. Isolated QC177-QC179 headings are exact.

INSERT INTO concept(concept_id,preferred_label)
VALUES (53343,'Atomic Physics Reference & Textbooks');

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal)
VALUES (53343,5032,0);

DELETE FROM lcc_range WHERE concept_id=5032;

INSERT INTO lcc_range(start_code,end_code,concept_id)
VALUES ('QC170','QC171.2',53343);

-- Wholly contained by broader ranges on the same destination.
DELETE FROM lcc_range
WHERE (concept_id,start_code,end_code) IN (
  (7300,'QC173.45','QC173.458'),
  (9550,'QC174.45','QC174.52')
);

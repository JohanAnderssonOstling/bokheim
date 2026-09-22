-- Split the acoustics schedule between its opening reference/study material and
-- the existing Acoustics Topics leaf. Exact history and mathematical headings
-- continue to override the opening range.

INSERT INTO concept(concept_id,preferred_label)
VALUES (53344,'Acoustics Reference, Study & Laboratories');

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal)
VALUES (53344,5031,0);

DELETE FROM lcc_range WHERE concept_id=5031;

INSERT INTO lcc_range(start_code,end_code,concept_id)
VALUES
  ('QC221','QC230.Z',53344),
  ('QC231','QC246',10017);
